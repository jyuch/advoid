# EDNS DNSSEC OK (DO) Flag Set Unconditionally

## Severity: Medium

## Location

`src/dns.rs:254`

```rust
resp_edns.set_dnssec_ok(true);
```

## Violated RFCs

- RFC 6891, Section 6.1.4
- RFC 3225, Section 3

## Description

RFC 3225 Section 3 states: "The DO bit of the query MUST be copied in the response." The server unconditionally sets DO=1 in all EDNS responses regardless of whether the client set it in the request. This signals to the client that the server supports DNSSEC, even though no DNSSEC validation is implemented.

## Impact

DNSSEC-aware stub resolvers or validating resolvers downstream may expect RRSIG records and other DNSSEC data that this server will never provide. This could cause validation failures or unexpected behavior in DNSSEC-aware clients.

## Suggested Fix

Copy the DO bit from the request's EDNS OPT record:

```rust
resp_edns.set_dnssec_ok(req_edns.dnssec_ok());
```
