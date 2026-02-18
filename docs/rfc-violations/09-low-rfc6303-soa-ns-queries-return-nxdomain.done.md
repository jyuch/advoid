# RFC 6303 Zones: SOA/NS Queries at Zone Apex Return NXDOMAIN

## Severity: Low-Medium

## Location

`src/dns.rs:147-150`

```rust
let upstream_response = if self.block_local_zone && is_rfc6303_zone(&name_str) {
    debug!("Blocking RFC 6303 local zone query {}", &name);
    metrics::counter!("dns_requests_block").increment(1);
    None
```

## Violated RFCs

- RFC 6303, Sections 4.1-4.4

## Description

RFC 6303 specifies that locally served zones should serve proper SOA and NS records for the zone apex. Specifically:

- A SOA query for a locally served zone (e.g., `SOA 10.in-addr.arpa.`) should return a synthetic SOA record, not NXDOMAIN.
- An NS query for a locally served zone should return a synthetic NS record.
- Only queries for names *within* the zone (that are not the zone apex itself) should return NXDOMAIN (with SOA in the authority section).

The current implementation returns NXDOMAIN for all queries to RFC 6303 zones, including SOA and NS queries at the zone apex.

## Impact

Resolvers checking for zone delegation or performing DNSSEC validation could be confused by receiving NXDOMAIN for the zone apex. In practice, most clients only issue PTR queries for these zones, so real-world impact is limited.

## Suggested Fix

For RFC 6303 zones, check whether the query is for the zone apex (SOA or NS record type). If so, return a synthetic SOA or NS record. For all other queries within the zone, return NXDOMAIN with the synthetic SOA in the authority section.
