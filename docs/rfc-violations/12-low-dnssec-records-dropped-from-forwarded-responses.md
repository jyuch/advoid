# SIG/DNSSEC Records Dropped from Forwarded Responses

## Severity: Low

## Location

`src/dns.rs:164-176`

```rust
Some(response) => {
    let mut response_header = Header::response_from_request(request.header());
    response_header.set_recursion_available(response.recursion_available());
    response_header.set_response_code(response.response_code());

    let response = response_builder.build(
        response_header,
        response.answers(),
        response.name_servers(),
        &[],        // <-- SIG records dropped
        response.additionals(),
    );
```

## Violated RFCs

- RFC 4035 (DNSSEC Protocol Modifications)

## Description

The third-to-last argument to `build()` is `&[]` (empty), which maps to the SIG/signature records section. If the upstream resolver returns DNSSEC data (RRSIG, DNSKEY, DS, NSEC, etc.) in this section, it will be silently dropped.

## Impact

Only relevant if the upstream returns DNSSEC signatures and the client expects them. Since advoid does not perform DNSSEC validation and unconditionally sets DO=1 (see issue #04), the combination means clients may expect DNSSEC data that is never delivered.

## Suggested Fix

Forward the SIG records from the upstream response instead of using an empty slice. Alternatively, if DNSSEC support is intentionally not provided, ensure the DO flag is not set in responses (see issue #04).
