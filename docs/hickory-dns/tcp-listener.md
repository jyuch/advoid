# TCP Listener Registration

Notes on adding TCP transport support with hickory-server's `Server`. Current for **hickory 0.26**.

## Server

`hickory_server::Server` manages both UDP and TCP transports. A single `Server` instance can serve the same `RequestHandler` over multiple transports simultaneously.

> **Renamed in 0.26:** `ServerFuture<T>` → `Server<T>`. Also, `register_socket_std` and `register_listener_std` were removed — convert with `tokio::net::UdpSocket::from_std` / `TcpListener::from_std` yourself.

### UDP Registration

```rust
use tokio::net::UdpSocket;

let socket = UdpSocket::bind("127.0.0.1:53").await?;
let mut server = Server::new(handler);
server.register_socket(socket);
```

### TCP Registration

```rust
use std::time::Duration;
use tokio::net::TcpListener;

let tcp_listener = TcpListener::bind("127.0.0.1:53").await?;
server.register_listener(tcp_listener, Duration::from_secs(5), 32);
```

- **2nd argument — idle timeout.** TCP connections with no activity for this duration are closed by the server.
- **3rd argument — `response_buffer_size`** (new in 0.26). The outgoing-response buffer length per connection, forwarded to `TcpStream::from_stream_with_buffer_size`. 0.25 called `TcpStream::from_stream`, which used hickory's internal `DEFAULT_STREAM_BUFFER_SIZE = 32`, so **pass `32` to preserve 0.25 behavior**. A larger value helps only under high per-connection message rates, where a small buffer would drop responses to backpressure.

### UDP + TCP on the Same Address

UDP and TCP use separate OS sockets, so the same `SocketAddr` can be used for both without conflict. This is the standard DNS behavior (RFC 7766).

```rust
let addr = "127.0.0.1:53";
let udp_socket = UdpSocket::bind(addr).await?;
let tcp_listener = TcpListener::bind(addr).await?;

let mut server = Server::new(handler);
server.register_socket(udp_socket);
server.register_listener(tcp_listener, Duration::from_secs(5), 32);
```

### Other 0.26 signature changes

- `with_access` takes `impl IntoIterator<Item = IpNet>` instead of `&[IpNet]`.
- `block_until_done` and `shutdown_gracefully` return `Result<(), NetError>` instead of `Result<(), ProtoError>`.
- New free function `default_tls_server_config(protocol, server_cert_resolver) -> io::Result<ServerConfig>`.

## TCP Framing

DNS over TCP requires a 2-byte length prefix before each message (RFC 1035 Section 4.2.2). `register_listener()` handles this framing transparently — the `RequestHandler` implementation does not need to be aware of the underlying transport.

## RequestHandler Is Transport-Agnostic

The `RequestHandler` trait receives parsed `Request` objects regardless of transport. No protocol-specific branching is needed in the handler. The same handler instance serves both UDP and TCP clients.

## Idle Timeout Considerations

The timeout passed to `register_listener()` controls how long an idle TCP connection is kept open. Considerations:

- Too short: clients performing multiple queries over a single connection may get disconnected prematurely
- Too long: idle connections consume server resources (file descriptors, memory)
- RFC 7766 Section 6.2.3 recommends servers SHOULD allow idle connections to remain open for a period on the order of seconds

A value of 5 seconds is a reasonable default for a local stub resolver.

## Shutdown

`Server::shutdown_token()` returns a `CancellationToken` that, when cancelled, shuts down both UDP and TCP listeners. No separate shutdown handling is needed per transport.

```rust
let shutdown_token = server.shutdown_token().clone();
// ...
shutdown_token.cancel();  // stops both UDP and TCP
```
