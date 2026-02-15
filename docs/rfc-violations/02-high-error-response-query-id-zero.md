# Error Responses Use Query ID 0

## Severity: High

## Location

`src/dns.rs:319-321`

```rust
let mut header = Header::new();
header.set_response_code(ResponseCode::ServFail);
header.into()
```

## Violated RFCs

- RFC 1035, Section 4.1.1

## Description

When an error occurs (upstream timeout, parse failure, etc.), the server constructs a SERVFAIL response using `Header::new()`, which creates a header with ID=0. RFC 1035 Section 4.1.1 requires that the response ID MUST match the query ID so that the client can correlate the response with its outstanding query.

## Impact

Clients will discard responses with mismatched IDs. Instead of receiving a prompt SERVFAIL (which would trigger immediate fallback behavior), clients will time out waiting for a response that never arrives. This degrades user experience during upstream failures.

## Suggested Fix

Use `Header::response_from_request(request.header())` instead of `Header::new()` to preserve the original query ID (and other required fields) in error responses.
