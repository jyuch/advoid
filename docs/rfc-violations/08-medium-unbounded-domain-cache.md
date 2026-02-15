# Unbounded CheckedDomain Cache (DoS Risk)

## Severity: Medium

## Location

`src/dns.rs:57-69`

```rust
struct CheckedDomain {
    block: FxHashSet<String>,
    allow: FxHashSet<String>,
}
```

## Related RFCs

- RFC 8906 (A Common Operational Problem in DNS Servers: Failure to Communicate)

## Description

The `CheckedDomain` cache stores every unique queried domain indefinitely with no eviction policy or size limit. Both the `block` and `allow` hash sets grow without bound as new domains are queried.

While not a direct protocol violation, RFC 8906 provides guidance on resolver robustness. An attacker performing random subdomain attacks (e.g., `random1.example.com`, `random2.example.com`, ...) can cause unbounded memory growth, eventually exhausting server memory.

## Impact

A targeted attack using random subdomains can cause out-of-memory conditions, resulting in denial of service. Even normal operation over long periods without restarts could lead to significant memory consumption.

## Suggested Fix

Implement a cache eviction strategy, such as:

- LRU cache with a maximum size limit
- TTL-based expiration
- Periodic cache clearing on a timer
