# RA Flag Not Set on Locally Generated NXDOMAIN Responses

## Severity: Low

## Location

`src/dns.rs:178-181`

## Violated RFCs

- RFC 1035, Section 4.1.1

## Description

RFC 1035 Section 4.1.1: "RA -- Recursion Available -- this bit is set or cleared in a response, and denotes whether recursive query support is available in the name server."

Since advoid acts as a recursive resolver (it forwards queries to an upstream resolver), it should set RA=1 in all responses. For forwarded responses (line 166), RA is correctly copied from the upstream response. However, for locally generated NXDOMAIN responses (blocked domains, RFC 6303 zones), the RA flag is not explicitly set.

## Impact

Most modern clients do not strictly check the RA flag, so practical impact is minimal. However, some strict implementations may interpret RA=0 as the server not supporting recursion and attempt to use a different resolver.

## Suggested Fix

Explicitly set `response_header.set_recursion_available(true)` when building locally generated NXDOMAIN responses.
