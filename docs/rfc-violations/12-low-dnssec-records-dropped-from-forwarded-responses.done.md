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

## Resolution

Fixed by passing `response.sig0()` in place of `&[]`, then **revised during the hickory 0.26 upgrade** — the slot is empty again, deliberately.

Two corrections to the analysis above:

1. **The third-to-last `build()` argument is not a "SIG/signature records section."** It is the authority-section SOA slot; `authorities` and `soa` are both emitted into the authority section. In 0.25 the transaction signature was a separate `sig0: Vec<Record>` field on `MessageResponse`, set via `set_sig0` — not reachable through `build()` at all. So the original fix put SIG(0) records into the authority section, which was not what was intended.

2. **The actual RFC 4035 concern was never present.** DNSSEC records (RRSIG, DNSKEY, DS, NSEC, NSEC3) travel in the ordinary answer / authority / additional sections, all three of which advoid forwards verbatim. `sig0()` returned SIG(0) *transaction* signatures (RFC 2931), which are a different mechanism and are always empty for a plain UDP stub query.

In hickory 0.26 `DnsResponse::sig0()` was removed and the signature became a single typed field:

```rust
pub signature: Option<Box<Record<TSIG>>>
```

Its element type is `Record<TSIG>`, not `Record<RData>`, so it can no longer be passed to any `build()` section argument; it has its own channel, `MessageResponse::set_signature`. Forwarding it would also be *wrong*: TSIG is a hop-by-hop signature whose MAC covers the upstream's message bytes, so attaching the upstream's signature to a response advoid rebuilt itself produces an invalid signature that a validating client would reject.

Current behavior (`src/dns.rs`, `handle_query`): the `soa` slot is `&[] as &[Record]` for forwarded responses and the signature is left unset. The DO-flag interaction noted under **Impact** is tracked separately as issue #04.
