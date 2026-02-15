# NXDOMAIN Response Missing AA (Authoritative Answer) Flag

## Severity: Medium

## Location

`src/dns.rs:178-181`

```rust
None => {
    let response = response_builder.error_msg(request.header(), ResponseCode::NXDomain);
    send_response(response_edns, response, response_handle).await?
}
```

## Violated RFCs

- RFC 1035, Section 4.1.1
- RFC 2308, Section 2.1

## Description

When the server makes an authoritative determination that a domain does not exist (blocklist match or RFC 6303 zone block), the AA (Authoritative Answer) bit should be set to 1 in the response header. The `error_msg` method uses the request header as the basis, which has AA=0. Since the blocker is the authority deciding this domain does not exist, the AA flag should be set.

RFC 2308 Section 2.1 describes NXDOMAIN responses from "authoritative" servers, implying the AA bit should be set.

## Impact

Clients and caches that check the AA bit may treat these NXDOMAIN responses as non-authoritative, affecting caching behavior and potentially causing the client to retry the query with other resolvers.

## Suggested Fix

Set the AA flag in the response header before building the NXDOMAIN response for blocked domains and RFC 6303 zones.
