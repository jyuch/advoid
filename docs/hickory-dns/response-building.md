# Building DNS Responses

Notes on building DNS responses with Hickory DNS.

## MessageResponseBuilder

`hickory_server::authority::MessageResponseBuilder` is the central type for response construction.

### Creation

```rust
use hickory_server::authority::MessageResponseBuilder;

let response_builder = MessageResponseBuilder::from_message_request(request);
```

### error_msg — Simple Error Responses

Generates a response with only a header and response code. No authority section is included.

```rust
let response = response_builder.error_msg(request.header(), ResponseCode::NotImp);
```

Use for: NotImp, ServFail, and other error responses that don't need a SOA record.

### build — Full Response

Explicitly specifies each section of the response.

```rust
let response = response_builder.build(
    response_header,        // Header
    &[] as &[Record],       // answers
    &[] as &[Record],       // name_servers
    &[soa_record],          // soa (authority section)
    &[] as &[Record],       // additionals
);
```

**Caveat: Lifetimes**

Arrays passed to `build()` must outlive the returned `MessageResponse`. Passing temporary arrays inline causes a lifetime error.

```rust
// BAD: temporary value does not live long enough
let response = response_builder.build(header, &[], &[], &[soa_record], &[]);
send_response(response).await?  // error[E0716]: temporary value dropped while borrowed

// OK: extend lifetime with a let binding
let soa_records = [soa_record];
let response = response_builder.build(header, &[], &[], &soa_records, &[]);
send_response(response).await?
```

When passing empty slices, a type annotation like `&[] as &[Record]` may be required when type inference cannot determine the element type.

## Header

```rust
use hickory_proto::op::Header;

let mut header = Header::response_from_request(request.header());
header.set_response_code(ResponseCode::NXDomain);
header.set_authoritative(true);  // AA flag
header.set_recursion_available(true);  // RA flag
```

## Building a SOA Record

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
record.set_dns_class(DNSClass::IN);
```

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
record.set_dns_class(DNSClass::IN);
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
| `name_servers` | Authority | NS records for referrals, SOA in NXDOMAIN/NODATA responses |
| `soa` | Authority | SOA record (shorthand for authority section) |
| `additionals` | Additional | Glue records (A/AAAA for NS targets) |

For **zone apex queries** (SOA or NS type at the zone name itself), the matching record goes in the **answers** section with `ResponseCode::NoError`:

```rust
let answer = synthetic_ns_record(zone);
let answers = [answer];
let response = response_builder.build(
    response_header,       // NoError, authoritative
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
    response_header,       // NXDomain, authoritative
    &[] as &[Record],      // no answers
    &[] as &[Record],
    &soa_records,          // SOA in authority section
    &[] as &[Record],
);
```

## DnsResponse — Accessing Upstream Response Sections

`hickory_proto::xfer::DnsResponse` wraps an upstream DNS response. It provides methods to access each section of the response message.

### Available Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `answers()` | `&[Record]` | Answer section records |
| `name_servers()` | `&[Record]` | Authority section records (NS, SOA) |
| `additionals()` | `&[Record]` | Additional section records (glue A/AAAA) |
| `sig0()` | `&[Record]` | SIG(0) transaction signature records (RFC 2931) |

**Note:** There is no `sigs()` method. The correct method name for signature records is `sig0()`.

### Forwarding an Upstream Response

When forwarding a response from an upstream resolver, map each `DnsResponse` method to the corresponding `build()` parameter:

```rust
let response = response_builder.build(
    response_header,
    response.answers(),        // answers → answers
    response.name_servers(),   // name_servers → name_servers
    response.sig0(),           // sig0 → soa (SIG(0) records)
    response.additionals(),    // additionals → additionals
);
```

Passing `&[]` instead of `response.sig0()` silently drops SIG(0) records from forwarded responses. This violates RFC 4035 if the upstream returns DNSSEC-related signature data.

### Header Fields to Forward

When constructing the response header for forwarded responses, copy relevant flags from the upstream:

```rust
let mut response_header = Header::response_from_request(request.header());
response_header.set_recursion_available(response.recursion_available());
response_header.set_response_code(response.response_code());
```

## send_response

Use `ResponseHandler`'s `send_response` to send the response to the client. For EDNS support, call `response.set_edns(edns)` beforehand.

```rust
async fn send_response<'a, R: ResponseHandler>(
    response_edns: Option<Edns>,
    mut response: MessageResponse<'_, 'a, ...>,
    mut response_handle: R,
) -> io::Result<ResponseInfo> {
    if let Some(resp_edns) = response_edns {
        response.set_edns(resp_edns);
    }
    response_handle.send_response(response).await
}
```
