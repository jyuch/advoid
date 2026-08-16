# The hickory-net Client (0.25 → 0.26)

Notes on the upstream-resolver client after the 0.26 crate reshuffle.

## hickory-proto was split; hickory-client is gone

In 0.26 `hickory-proto` holds **protocol data structures only** (`op`, `rr`, `serialize`, `dnssec`, `access_control`, `error`). Everything network-related moved into a **new crate, `hickory-net`**, and `hickory-client` was folded into it as `hickory_net::client`.

| 0.25 | 0.26 |
|------|------|
| `hickory_client::client::{Client, ClientHandle}` | `hickory_net::client::{Client, ClientHandle}` |
| `hickory_proto::runtime::TokioRuntimeProvider` | `hickory_net::runtime::TokioRuntimeProvider` |
| `hickory_proto::udp::UdpClientStream` | `hickory_net::udp::UdpClientStream` |
| `hickory_proto::xfer::DnsResponse` | `hickory_proto::op::DnsResponse` |
| `hickory_proto::xfer::*` (rest) | `hickory_net::xfer::*` |
| `hickory_proto::tcp` / `h2` / `h3` / `quic` / `rustls` | `hickory_net::*` |

`hickory-net` re-exports proto as `hickory_net::proto`, and hickory-server re-exports net as `hickory_server::net` — so `hickory_server::net::NetError` and `hickory_net::NetError` are the same type.

### Cargo features

`hickory-net`'s default feature set is `["tokio"]`, and `TokioRuntimeProvider`, `runtime::iocompat`, and `spawn_bg` are all gated behind it. **Do not use `default-features = false`** unless you add `features = ["tokio"]` back explicitly:

```toml
[dependencies.hickory-net]
version = "0.26"
```

hickory-server happens to declare `hickory-net` with `features = ["tokio"]`, so feature unification would turn it on anyway when both are dependencies — but relying on that is fragile.

The `client`, `udp`, `tcp`, `xfer`, and `runtime` modules themselves are unconditional, because they are generic over `RuntimeProvider`.

## Client is generic and Clone

```rust
#[derive(Clone)]
pub struct Client<P> { /* .. */ }
```

- **Generic over the runtime provider.** Write `Client<TokioRuntimeProvider>`. Struct fields and function signatures holding one need the type argument.
- **`Clone`.** `RuntimeProvider: Clone` is a supertrait, so `Client<TokioRuntimeProvider>: Clone` holds. Since `ClientHandle::query` takes `&mut self`, cloning per request is the idiomatic way to share one client — **a `Mutex` is not needed**:
  ```rust
  let mut upstream = self.upstream.clone();
  let response = upstream.query(name, query_class, query_type).await?;
  ```

## Construction became synchronous and infallible

`Client::connect` no longer exists. `new` / `with_timeout` / `from_sender` are all plain functions returning `(Self, DnsExchangeBackground<..>)` — no `async`, no `Result`. The `signer: Option<Arc<dyn MessageFinalizer>>` parameter is gone (signing is configured on the stream builder now).

`UdpClientStream::builder(..).build()` also changed: it returns the stream **directly** instead of a `UdpClientConnect` future.

```rust
// 0.25
let conn = UdpClientStream::builder(addr, TokioRuntimeProvider::new()).build();
let (client, background) = Client::connect(conn).await?;   // async + Result

// 0.26
let conn = UdpClientStream::builder(addr, TokioRuntimeProvider::new()).build();
let (client, background) = Client::<TokioRuntimeProvider>::from_sender(conn);
tokio::spawn(background);
```

Two gotchas:

- The turbofish `Client::<TokioRuntimeProvider>::from_sender` is **required** — `P` is not inferable from the `S: DnsRequestSender` argument.
- `DnsExchangeBackground`'s `Future::Output` changed from `Result<(), ProtoError>` to `()`, so `tokio::spawn(background)` works with no adapter.

There is also a convenience `UdpClientStreamBuilder::exchange()` that builds the stream *and* spawns the background task. New builder knobs: `with_max_retries(u8)`, `with_retry_interval_floor(u64)`, `with_os_port_selection`, `avoid_local_ports`.

## Querying

`ClientHandle` still exists, is still blanket-implemented for every `DnsHandle`, and still needs to be in scope to call `.query()`:

```rust
use hickory_net::client::{Client, ClientHandle};

// -> Result<DnsResponse, NetError>   (0.25: Result<DnsResponse, ClientError>)
let response = client.query(name, DNSClass::IN, RecordType::A).await?;
```

Reading the response is now field-based — see `response-building.md` for the full accessor table.

```rust
let answers = &response.answers;
for record in answers {
    if let RData::A(addr) = record.data { /* .. */ }
}
```

## NetError

`hickory_net::NetError` replaces the parts of `ProtoErrorKind` / `ClientError` that concerned transport. It is a `thiserror` enum, `Clone` (hence the internal `Arc`s), and `#[non_exhaustive]`, with variants including `Proto(ProtoError)`, `Dns(DnsError)`, `Io(Arc<io::Error>)`, `Timeout`, `Busy`, `NoConnections`. Helpers: `is_nx_domain()`, `is_no_records_found()`, `into_soa()`.

Conversions are **one-directional**:

- `impl From<io::Error> for NetError` exists (mapping `ErrorKind::TimedOut` to `NetError::Timeout`), so `?` on an `io::Error` works in a function returning `Result<_, NetError>`.
- **There is no `impl From<NetError> for io::Error`.** Any helper of yours declared `-> io::Result<T>` that forwards a `NetError` must change its return type (or convert explicitly with `io::Error::other(e)`).
- `NetError: Error + Send + Sync + 'static`, so `?` into `anyhow::Result` works.

`ProtoError` is now a flat `#[non_exhaustive]` enum, and `ProtoErrorKind`, `ForwardData`, `ENABLE_BACKTRACE`, and `ExtBacktrace` are no longer exported from hickory-proto. `ForwardNSData` moved to `hickory_net`.

## No packaged changelog

None of `hickory-proto`, `hickory-net`, or `hickory-server` 0.26.1 ship a `CHANGELOG.md` or migration guide in the published crate. The best in-tree references are:

- `hickory-net-0.26.1/src/client/tests.rs` — the working UDP client example (the README no longer carries it).
- `hickory-server-0.26.1/src/zone_handler/catalog.rs` — the reference `RequestHandler` implementation.
- `hickory-server-0.26.1/src/zone_handler/message_response.rs` — rustdoc example showing the new response idiom.
