# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

advoid is a DNS-based ad blocker written in Rust. It acts as a DNS stub resolver that intercepts queries, checks them against a blocklist, and returns NXDOMAIN for blocked domains while forwarding allowed queries to an upstream DNS resolver.

## Build and Development Commands

```bash
# Build
cargo build
cargo build --release    # LTO + single codegen unit + strip

# Test
cargo test
cargo test test_name     # Run a single test by name

# Run (minimum required arguments)
cargo run -- --bind 127.0.0.1:53 --upstream 1.1.1.1:53 --exporter 127.0.0.1:3000 --block path/to/blocklist.txt

# Docker
docker build -t advoid .
```

The project uses Rust edition 2024. Windows MSVC builds use static CRT linking (configured in `.cargo/config.toml`).

## Architecture

### Data Flow

1. DNS query arrives on the UDP socket or TCP listener (both bound to `--bind`)
2. RFC 6303 check: if `block_local_zone` is enabled, queries under private/local IP reverse zones are handled locally — an apex SOA/NS query is answered with a synthetic SOA/NS record (NOERROR), anything else returns NXDOMAIN with the matched zone's synthetic SOA in the authority section
3. Blocklist check: domain is matched against blocklist using label-boundary suffix matching (with a bounded LRU cache of past decisions)
4. If blocked: return NXDOMAIN (AA + RA set, synthetic SOA in authority section), increment `dns_requests_block` metric
5. If allowed: forward to upstream resolver, rebuild the response (answers, authorities, additionals forwarded; RA and RCODE copied from upstream), increment `dns_requests_forward` metric
6. Request/response events are sent to configured sink (S3/Databricks/stub) via unbounded channels
7. Background workers batch and upload events periodically

### Core Components

**`src/dns.rs`** — DNS request handler
- `StubRequestHandler`: Implements hickory-server's `RequestHandler` trait
- `find_rfc6303_zone()` / `is_rfc6303_zone()`: Match queries against private/local IP reverse zones per RFC 6303 (enabled by default, disable with `--forward-local-zone`)
- `synthetic_soa_record_for_zone()` / `synthetic_ns_record()`: Build the synthetic SOA/NS records used for NXDOMAIN authority sections and RFC 6303 zone apex answers (`advoid.` is the fallback zone name)
- `CheckedDomain`: Bounded `LruCache<String, bool>` (capacity from `--cache-cap`, default 10000) behind a `tokio::sync::Mutex`, caching both allow and block decisions to avoid repeated full blocklist scans
- EDNS support (version check → BADVERS, `dnssec_ok` echoed from the request, response max payload fixed at 1232), Prometheus metrics emission, event sink integration

**`src/event/`** — Event sink system (module directory)
- `mod.rs`: `Sink` trait, `Request`/`Response` types, re-exports all sink types
- `worker.rs`: `EventUploader` trait + generic `initialize_worker()` — shared batching, NDJSON serialization, sleep/cancel, and graceful drain logic. New sinks only need to implement `EventUploader`.
- `channel.rs`: `ChannelSink` — shared `Sink` impl for channel-based sinks (UUID generation + channel send). Sinks that use background workers wrap this and delegate.
- `stub.rs`: `StubSink` (no-op)
- `s3.rs`: `S3Sink`, `S3Uploader` (implements `EventUploader`), `s3_key()`
- `databricks.rs`: `DatabricksSink`, `DatabricksUploader`, `DatabricksClient` (OAuth token caching with automatic refresh)
- To add a new sink: implement `EventUploader`, create a struct wrapping `ChannelSink`, use `initialize_worker()` in the constructor

**`src/blocklist.rs`** — Loads blocklist from file path or HTTP(S) URL. Format: one domain per line, `#` comments, blank lines skipped; a trailing dot is appended to each entry to form a FQDN.

**`src/metrics.rs`** — Prometheus metrics server (axum, `/metrics` endpoint). Counters: `dns_requests_total`, `dns_requests_block`, `dns_requests_forward`. Gauge: `dns_cache_entries` (current size of the domain lookup cache, emitted from `src/dns.rs`).

**`src/trace.rs`** — Optional OpenTelemetry tracing via `--otel`. Falls back to stdout logging. `OtelInitGuard` ensures proper shutdown. Supports `--otel-api-key` (or `OTEL_API_KEY` env var) for backends requiring authentication (e.g., New Relic's `api-key` header). TLS is automatically enabled when the endpoint uses `https://`. `service.name` and `service.version` default to crate metadata but can be overridden via `--otel-service-name` and `--otel-service-version`.

**`src/main.rs`** — CLI (clap derive), component initialization, UDP socket + TCP listener registration on the hickory `Server`, graceful shutdown via ctrl-c + cancellation tokens (server first, then sink workers).

### Key Dependencies

- `hickory-server`/`hickory-net`/`hickory-proto`: DNS protocol (0.26; `hickory-client` was folded into `hickory-net`)
- `tokio`: Async runtime
- `axum`: Metrics HTTP server
- `aws-sdk-s3`: S3 event sink
- `reqwest`: HTTP client (blocklist fetching, Databricks API)
- `rustc-hash`: FxHashSet for fast blocklist lookups
- `lru`: Bounded LRU cache for domain lookup decisions
- `opentelemetry-otlp`: Optional distributed tracing

### Project Structure

The crate is split into a library (`src/lib.rs` re-exports all modules) and a binary (`src/main.rs`). Tests live in `#[cfg(test)]` modules within each source file.

### RFC Compliance Tracking

`docs/rfc-violations/` contains known RFC compliance issues, numbered and severity-tagged (e.g., `01-high-...`). When a violation is fixed, rename the file to `.done.md` (e.g., `02-high-error-response-query-id-zero.done.md`). Do not delete the file. All 12 currently tracked violations are marked `.done.md`.

### Hickory DNS Usage Notes

`docs/hickory-dns/` contains notes on how to use the Hickory DNS library (hickory-server, hickory-net, hickory-proto). When investigating or implementing Hickory DNS APIs, document the findings there for future reference.
- `client-migration.md`: 0.25 → 0.26 crate split and client API changes
- `response-building.md`: `MessageResponseBuilder` / `Metadata` usage and lifetime constraints
- `tcp-listener.md`: registering a TCP listener on `Server`

## Conventions

- Write commit messages in English, including the subject line and body

## Important Notes

- The blocklist is loaded once at startup into an `FxHashSet` and never reloaded
- Blocklist lookups are a linear scan over the whole set; the LRU cache (`--cache-cap`, default 10000 entries) is what keeps repeated queries cheap
- Event sinks use unbounded channels — be mindful of memory under high query loads
- Databricks credentials support both CLI args and environment variables (`DATABRICKS_HOST`, `DATABRICKS_CLIENT_ID`, `DATABRICKS_CLIENT_SECRET`, `DATABRICKS_VOLUME_PATH`); CLI args take precedence
- OTel API key can be set via `--otel-api-key` or `OTEL_API_KEY` env var; HTTPS endpoints automatically get TLS configured via tonic's `ClientTlsConfig`
- The release profile enables aggressive optimization: LTO, single codegen unit, binary stripping
