# NXDOMAIN Responses Missing SOA Record in Authority Section

## Severity: High

## Location

`src/dns.rs:178-181`

```rust
None => {
    let response = response_builder.error_msg(request.header(), ResponseCode::NXDomain);
    send_response(response_edns, response, response_handle).await?
}
```

## Violated RFCs

- RFC 1035, Section 4.3.1
- RFC 2308, Section 2.1 (Negative Caching of DNS Queries)

## Description

Blocked domains receive an NXDOMAIN response with no SOA record in the Authority Section. RFC 2308 Section 2.1 states: "Name servers authoritative for a zone MUST include the SOA record of the zone in the authority section of the response when reporting an NXDOMAIN." RFC 8020 (NXDOMAIN: There Really Is Nothing Underneath) Section 2 also reinforces that NXDOMAIN responses need proper authority data for caches to function correctly.

Since advoid acts as an authoritative source for blocked domains, it should synthesize a SOA record in the authority section.

## Impact

Downstream resolvers and caches cannot properly cache negative responses because the SOA `MINIMUM` field (negative TTL) is unavailable. This causes repeated queries for the same blocked domains, increasing load on both the client and the server.

## Suggested Fix

Generate a synthetic SOA record (e.g., with a reasonable negative TTL) and include it in the authority section of every NXDOMAIN response produced for blocked domains and RFC 6303 zone queries.
