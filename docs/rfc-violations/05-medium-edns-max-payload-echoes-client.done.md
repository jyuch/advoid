# EDNS max_payload Echoes Client Value Instead of Server's

## Severity: Medium

## Location

`src/dns.rs:255`

```rust
resp_edns.set_max_payload(req_edns.max_payload().max(512));
```

## Violated RFCs

- RFC 6891, Section 6.2.3

## Description

The `max_payload` in an EDNS response OPT record is meant to advertise the server's own maximum UDP payload size, not echo back the client's value. RFC 6891 Section 6.2.3: "The responder's UDP payload size [...] is the number of octets of the largest UDP payload that can be reassembled and delivered in the responder's network stack."

The current implementation echoes the client's value (clamped to minimum 512), which tells the client the server can handle whatever size the client requested.

## Impact

If a client advertises a very large payload size (e.g., 65535), the server echoes it back, implying it can handle responses of that size. This could lead to unexpected behavior in edge cases.

## Suggested Fix

Set a fixed server-side value. RFC 6891 and current best practice (DNS Flag Day 2020) recommend 1232 bytes:

```rust
resp_edns.set_max_payload(1232);
```
