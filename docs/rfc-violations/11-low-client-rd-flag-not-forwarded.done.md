# Client RD Flag Not Forwarded to Upstream

## Severity: Low

## Location

`src/dns.rs:119-130`

```rust
async fn forward_to_upstream(
    &self,
    name: Name,
    query_class: DNSClass,
    query_type: RecordType,
) -> anyhow::Result<DnsResponse> {
    let mut upstream = { self.upstream.lock().await.clone() };
    let response = upstream
        .query(name.clone(), query_class, query_type)
        .await?;
    Ok(response)
}
```

## Violated RFCs

- RFC 1035, Section 4.1.1

## Description

The RD (Recursion Desired) flag from the original client query is not forwarded to the upstream resolver. The hickory `Client::query` method likely sets RD=1 by default, but if a client sends a query with RD=0, the server should respect that and not request recursion from the upstream. Per RFC 1035 Section 4.1.1, the RD bit is "set in a query and is copied into the response."

## Impact

In practice, virtually all stub resolver clients set RD=1, so real-world impact is minimal. However, a strict implementation should respect the client's RD setting.

## Suggested Fix

Pass the client's RD flag through to the upstream query, or at minimum, verify that the upstream query respects the original client's RD setting.
