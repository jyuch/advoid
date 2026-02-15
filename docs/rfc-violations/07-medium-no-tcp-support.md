# No TCP Transport Support

## Severity: Medium

## Location

`src/main.rs:184-197`

```rust
let conn = UdpClientStream::builder(opt.upstream, TokioRuntimeProvider::new()).build();
let (upstream, background) = Client::connect(conn).await?;
// ...
let socket = UdpSocket::bind(&opt.bind).await?;
let mut server = ServerFuture::new(handler);
server.register_socket(socket);
```

## Violated RFCs

- RFC 1035, Section 4.2
- RFC 7766 (DNS Transport over TCP)

## Description

RFC 7766 states: "DNS implementations MUST support both UDP and TCP transport." The server only listens on UDP and only forwards queries via UDP.

If a response exceeds the EDNS payload size and needs to be truncated (TC=1), the client should be able to retry over TCP, but this server has no TCP listener. Additionally, if the upstream response is truncated, the server has no mechanism to retry the query over TCP.

## Impact

Responses larger than the UDP payload size (e.g., DNSSEC-signed responses, large TXT records, SPF records with many includes) will be truncated with no TCP fallback. This violates a MUST-level requirement in RFC 7766.

## Suggested Fix

Add a TCP listener alongside the UDP socket using `ServerFuture::register_listener`. Also consider using a TCP-capable upstream client (or a retry mechanism that falls back to TCP when TC=1 is received).
