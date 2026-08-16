# Building DNS Responses

Notes on building DNS responses with Hickory DNS. Current for **hickory 0.26**; 0.25 differences are called out where they bite.

## MessageResponseBuilder

`hickory_server::zone_handler::MessageResponseBuilder` is the central type for response construction.

> **Moved in 0.26:** the module `hickory_server::authority` was renamed to `hickory_server::zone_handler` (and the `Authority` trait to `ZoneHandler`).

### Creation

```rust
use hickory_server::zone_handler::MessageResponseBuilder;

// No EDNS
let response_builder = MessageResponseBuilder::from_message_request(request);

// With EDNS — `new` is public as of 0.26 and takes the response EDNS directly
let response_builder = MessageResponseBuilder::new(&request.queries, response_edns);
```

`from_message_request(request)` is exactly `Self::new(&message.queries, None)`. Prefer `new` whenever a response EDNS exists — see [EDNS is borrowed in 0.26](#edns-is-borrowed-in-026).

### error_msg — Simple Error Responses

Generates a response with only a header and response code. No authority section is included.

```rust
let response = response_builder.error_msg(&request.metadata, ResponseCode::NotImp);
```

Takes `&Metadata` in 0.26 (was `&Header`). Internally it calls `Metadata::response_from_request` and sets the response code. Use for NotImp, ServFail, and other error responses that don't need a SOA record.

### build — Full Response

Explicitly specifies each section of the response. The first argument is a `Metadata`, **not** a `Header`.

```rust
let response = response_builder.build(
    metadata,               // Metadata (was Header in 0.25)
    &[] as &[Record],       // answers
    &[] as &[Record],       // authorities  (renamed from `name_servers`)
    &[soa_record],          // soa (authority section)
    &[] as &[Record],       // additionals
);
```

**Caveat: Lifetimes**

Arrays passed to `build()` must outlive the returned `MessageResponse`. Passing temporary arrays inline causes a lifetime error.

```rust
// BAD: temporary value does not live long enough
let response = response_builder.build(metadata, &[], &[], &[soa_record], &[]);
send_response(response).await?  // error[E0716]: temporary value dropped while borrowed

// OK: extend lifetime with a let binding
let soa_records = [soa_record];
let response = response_builder.build(metadata, &[], &[], &soa_records, &[]);
send_response(response).await?
```

When passing empty slices, a type annotation like `&[] as &[Record]` may be required when type inference cannot determine the element type. `&Vec<Record>` satisfies the `IntoIterator<Item = &Record>` bound directly, so upstream response sections can be passed without converting to a slice.

`build` consumes the builder and the builder is not `Clone`, so construct one builder per response rather than trying to share one across `match` arms.

## Header / Metadata

`Header` was split in 0.26:

```rust
pub struct Header {
    pub metadata: Metadata,     // id, flags, op_code, response_code
    pub counts: HeaderCounts,   // record counts, used during encoding
}
```

`Header::response_from_request` and **all** `set_*` setters are gone. `Metadata` is `#[non_exhaustive]` with public fields, so obtain one from a constructor and then assign fields:

```rust
use hickory_proto::op::{Metadata, ResponseCode};

let mut metadata = Metadata::response_from_request(&request.metadata);
metadata.response_code = ResponseCode::NXDomain;
metadata.authoritative = true;         // AA flag
metadata.recursion_available = true;   // RA flag
```

| 0.25 | 0.26 |
|------|------|
| `Header::response_from_request(h)` | `Metadata::response_from_request(m)` |
| `header.set_response_code(c)` | `metadata.response_code = c` |
| `header.set_authoritative(true)` | `metadata.authoritative = true` |
| `header.set_recursion_available(b)` | `metadata.recursion_available = b` |

`Header` derefs to `Metadata`, so `&header` coerces where `&Metadata` is expected. `Header` itself is *not* `#[non_exhaustive]`, so the struct literal is constructible from downstream crates — which is how you build a `ResponseInfo` by hand:

```rust
use hickory_proto::op::{Header, HeaderCounts, Metadata, ResponseCode};

// `ResponseInfo::serve_failed` is pub(crate) in hickory-server; roll your own:
fn serve_failed(request: &Request) -> ResponseInfo {
    let mut metadata = Metadata::response_from_request(&request.metadata);
    metadata.response_code = ResponseCode::ServFail;
    Header { metadata, counts: HeaderCounts::default() }.into()
}
```

## Request accessors are fields in 0.26

`Request` derefs to `MessageRequest`, whose getters became public fields.

| 0.25 | 0.26 |
|------|------|
| `request.header()` | `&request.metadata` |
| `request.edns()` | `request.edns.as_ref()` |
| `request.message_type()` | `request.metadata.message_type` |
| `request.op_code()` | `request.metadata.op_code` |
| `request.raw_queries()` | `&request.queries` |

`request.request_info()` still exists, but its error type changed from `ProtoError` to `hickory_server::zone_handler::LookupError`, and `RequestInfo`'s `header` field was renamed:

```rust
let op_code = request_info.metadata.op_code;   // was request_info.header.op_code()
```

## Building a SOA Record

`Record`'s getters/setters became public fields in 0.26 — `set_dns_class` and friends are gone. (The file also moved: `rr/resource.rs` → `rr/record.rs`.)

```rust
use hickory_proto::rr::rdata::SOA;
use hickory_proto::rr::{DNSClass, Name, RData, Record};

let mname = Name::from_ascii("ns.example.").unwrap();
let rname = Name::from_ascii("hostmaster.example.").unwrap();
let soa = SOA::new(
    mname,
    rname,
    1,      // serial
    3600,   // refresh
    1800,   // retry
    604800, // expire
    3600,   // minimum (negative cache TTL)
);

let mut record = Record::from_rdata(
    Name::from_ascii("example.").unwrap(),  // owner name
    3600,                                    // TTL
    RData::SOA(soa),
);
record.dns_class = DNSClass::IN;   // was record.set_dns_class(DNSClass::IN)
```

`from_rdata` already defaults `dns_class` to `DNSClass::IN`, so that assignment is redundant — keep it only to make intent explicit.

Note `RecordSet` (`hickory_proto::rr::RecordSet`) keeps private fields and therefore *still* has `set_dns_class` / `set_ttl`. Don't confuse the two.

### SOA Parameters

| Parameter | Description |
|-----------|-------------|
| mname | Primary nameserver |
| rname | Admin email address (with `@` replaced by `.`) |
| serial | Zone serial number |
| refresh | Interval for secondaries to attempt zone transfer |
| retry | Retry interval after zone transfer failure |
| expire | Time before secondaries consider the zone invalid |
| minimum | Negative cache TTL (RFC 2308) |

## Building an NS Record

`NS` is a newtype struct wrapping `Name`, defined in `hickory_proto::rr::rdata::name`.

```rust
use hickory_proto::rr::rdata::NS;
use hickory_proto::rr::{DNSClass, Name, RData, Record};

let ns_name = Name::from_ascii("ns.example.").unwrap();
let mut record = Record::from_rdata(
    Name::from_ascii("example.").unwrap(),  // owner name (zone apex)
    3600,                                    // TTL
    RData::NS(NS(ns_name)),                 // NS is a newtype: NS(Name)
);
record.dns_class = DNSClass::IN;
```

### Import Path

`NS` can be imported from either location:

- `hickory_proto::rr::rdata::NS` (re-exported)
- `hickory_proto::rr::rdata::name::NS` (original definition)

The same newtype pattern applies to `CNAME`, `PTR`, and `ANAME` — they are all `pub struct T(pub Name)` defined in the same `name` module.

## Placing Records in Response Sections

The `build()` method maps its arguments to DNS message sections:

| Argument | DNS Section | Typical Use |
|----------|-------------|-------------|
| `answers` | Answer | Records that directly answer the query (A, AAAA, SOA at apex, NS at apex) |
| `authorities` | Authority | NS records for referrals, SOA in NXDOMAIN/NODATA responses |
| `soa` | Authority | SOA record (shorthand for authority section) |
| `additionals` | Additional | Glue records (A/AAAA for NS targets) |

`authorities` and `soa` are both emitted into the authority section.

For **zone apex queries** (SOA or NS type at the zone name itself), the matching record goes in the **answers** section with `ResponseCode::NoError`:

```rust
let answer = synthetic_ns_record(zone);
let answers = [answer];
let response = response_builder.build(
    metadata,              // NoError, authoritative
    &answers,              // NS/SOA record as answer
    &[] as &[Record],
    &[] as &[Record],
    &[] as &[Record],
);
```

For **NXDOMAIN responses**, the SOA record goes in the **soa/authority** section with no answers:

```rust
let soa_records = [soa_record];
let response = response_builder.build(
    metadata,              // NXDomain, authoritative
    &[] as &[Record],      // no answers
    &[] as &[Record],
    &soa_records,          // SOA in authority section
    &[] as &[Record],
);
```

## DnsResponse — Accessing Upstream Response Sections

`DnsResponse` **moved from `hickory_proto::xfer` to `hickory_proto::op`** in 0.26 (the whole `xfer` module left hickory-proto; see `client-migration.md`).

Its section accessors became public fields. `DnsResponse` derefs to `Message`, and `Message` derefs to `Metadata`, so header flags are reachable in one hop.

| 0.25 method | 0.26 |
|-------------|------|
| `response.answers()` | `&response.answers` |
| `response.name_servers()` | `&response.authorities` (**renamed**) |
| `response.additionals()` | `&response.additionals` |
| `response.sig0()` | **removed** — see below |
| `response.response_code()` | `response.metadata.response_code` |
| `response.recursion_available()` | `response.metadata.recursion_available` |

### sig0() is gone

0.25 exposed `sig0() -> &[Record]`. In 0.26 the transaction signature is a single typed field:

```rust
pub signature: Option<Box<Record<TSIG>>>
```

Because the element type is `Record<TSIG>` rather than `Record<RData>`, it **cannot** be passed to any of `build()`'s section arguments. It travels on its own channel via `MessageResponse::set_signature(Box<Record<TSIG>>)`.

For a forwarding stub resolver the right move is to not forward it at all: TSIG is a hop-by-hop signature whose MAC covers the upstream's message, so attaching it to a response you rebuilt yourself yields an invalid signature. Pass an empty slice for `soa` and leave the signature unset. Real DNSSEC records (RRSIG, NSEC, DNSKEY, DS) live in the answer/authority/additional sections and are forwarded normally.

### Forwarding an Upstream Response

```rust
let mut metadata = Metadata::response_from_request(&request.metadata);
metadata.recursion_available = upstream.metadata.recursion_available;
metadata.response_code = upstream.metadata.response_code;

let response = response_builder.build(
    metadata,
    &upstream.answers,
    &upstream.authorities,
    &[] as &[Record],        // no TSIG forwarding
    &upstream.additionals,
);
```

Avoid shadowing the `DnsResponse` binding with the `MessageResponse` (`let response = response_builder.build(metadata, &response.answers, ...)`). It borrow-checks, but field access on a shadowed name reads badly — name the upstream one `upstream`.

## EDNS is borrowed in 0.26

This is the change most likely to force a restructure. In 0.25 EDNS was passed by value:

```rust
// 0.25
pub fn edns(&mut self, edns: Edns) -> &mut Self
pub fn set_edns(&mut self, edns: Edns) -> &mut Self
```

In 0.26 both borrow for the builder's `'q` lifetime:

```rust
// 0.26
pub fn new(queries: &'q Queries, edns: Option<&'q Edns>) -> Self
pub fn edns(&mut self, edns: &'q Edns) -> &mut Self
pub fn set_edns(&mut self, edns: &'q Edns) -> &mut Self
```

Consequences:

1. **The response `Edns` must outlive every `MessageResponse` built from it.** Own it in the outermost frame and pass `Option<&Edns>` downward. `Option<&Edns>` is `Copy`, so one value can feed several builders.
2. **Prefer attaching EDNS at construction time** via `MessageResponseBuilder::new(&request.queries, response_edns)`. `builder.edns()` returns `&mut Self` and so cannot be chained into the consuming `build` / `build_no_records` / `error_msg`, forcing a two-statement dance at every call site.
3. `MessageResponseBuilder<'q>` and `MessageResponse<'q, ..>` are **covariant in `'q`**, so `'q` is inferred as the intersection of the request and EDNS borrows. No named lifetimes are needed in your own signatures.
4. `MessageResponse::encode` reads `edns.max_payload()` to size the UDP encoder. Setting EDNS at construction makes it structurally impossible to build a response that silently lost its OPT record.

The canonical pattern (mirroring `Catalog::handle_request`) is to hoist a deferred-init binding:

```rust
// Must outlive every MessageResponse built downstream.
let mut resp_edns: Edns;

let response_edns: Option<&Edns> = if let Some(req_edns) = request.edns.as_ref() {
    resp_edns = Edns::new();
    resp_edns.set_dnssec_ok(req_edns.flags().dnssec_ok);
    resp_edns.set_max_payload(1232);
    resp_edns.set_version(0);

    if req_edns.version() > 0 {
        // Mutate while still owned; the order matters — this must precede the shared borrow.
        resp_edns.set_rcode_high(ResponseCode::BADVERS.high());

        let mut metadata = Metadata::response_from_request(&request.metadata);
        metadata.response_code = ResponseCode::BADVERS;

        let response = MessageResponseBuilder::new(&request.queries, Some(&resp_edns))
            .build_no_records(metadata);
        return response_handle.send_response(response).await.unwrap_or_else(|_| serve_failed(request));
    }

    Some(&resp_edns)
} else {
    None
};
```

Deferred init — declaring `let mut resp_edns: Edns;` and assigning it only inside one branch, then borrowing out of that branch — does borrow-check; this is upstream's own pattern.

Note that `Catalog::send_error_response` clones into a second `Edns` before calling `set_rcode_high`, but only because it *receives* `Option<&Edns>` and cannot mutate through a shared reference. If you own the value at that point, mutate in place.

Also new: `hickory_proto::op::DEFAULT_MAX_PAYLOAD_LEN` (= 1232) can replace a hardcoded literal.

## send_response

`ResponseHandler::send_response` now returns **`Result<ResponseInfo, hickory_net::NetError>`** (was `io::Result<ResponseInfo>`).

There is `impl From<io::Error> for NetError`, but **no** `impl From<NetError> for io::Error`, so a helper declared `-> io::Result<ResponseInfo>` will no longer compile. Mirror the trait's error type instead. `NetError` is a `thiserror` enum that is `Send + Sync + 'static`, so `?` into `anyhow::Result` works.

Because EDNS is attached at builder construction, the helper reduces to a thin wrapper:

```rust
async fn send_response<'a, R: ResponseHandler>(
    response: MessageResponse<
        '_,
        'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
    >,
    mut response_handle: R,
) -> Result<ResponseInfo, NetError> {
    response_handle.send_response(response).await
}
```

## ResponseInfo

```rust
#[repr(transparent)]
pub struct ResponseInfo(Header);
```

- Still built via `impl From<Header>`, so `header.into()` works — but you must assemble a full `Header { metadata, counts }`.
- `Deref` target changed from `Header` (0.25) to `Metadata` (0.26): `response_info.response_code()` becomes the field `response_info.response_code`.
- Record counts moved to a method: `response_info.counts().answers` / `.authorities` / `.additionals` / `.queries` (0.25 had `answer_count()`, `name_server_count()`, `additional_count()`).
- `ResponseInfo::serve_failed` is `pub(crate)` — not usable downstream. Build one by hand as shown under [Header / Metadata](#header--metadata).

## RequestHandler gained a second generic

```rust
#[async_trait::async_trait]
pub trait RequestHandler: Send + Sync + Unpin + 'static {
    async fn handle_request<R: ResponseHandler, T: Time>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo;
}
```

`T` is `hickory_net::runtime::Time` (also reachable as `hickory_server::net::runtime::Time`). It is a static-dispatch-only parameter — there is no `T` argument — provided so handlers can call `T::current_time()` instead of reading the clock directly. Leaving it unused in the body is fine and warning-free. The server always instantiates it as `TokioTime`, so nothing changes at the `Server::new` call site.
