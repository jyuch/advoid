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
