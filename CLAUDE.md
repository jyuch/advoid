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

1. DNS query arrives at UDP socket (bound via `--bind`)
2. RFC 6303 check: if `block_local_zone` is enabled, PTR queries for private/local IP reverse zones return NXDOMAIN immediately
3. Blocklist check: domain is matched against blocklist using suffix matching (with a two-level allow/block cache)
4. If blocked: return NXDOMAIN, increment `dns_requests_block` metric
5. If allowed: forward to upstream resolver, return response, increment `dns_requests_forward` metric
6. Request/response events are sent to configured sink (S3/Databricks/stub) via unbounded channels
7. Background workers batch and upload events periodically

### Core Components

**`src/dns.rs`** — DNS request handler
- `StubRequestHandler`: Implements hickory-server's `RequestHandler` trait
- `is_rfc6303_zone()`: Blocks PTR queries for private/local IP reverse zones per RFC 6303 (enabled by default, disable with `--forward-local-zone`)
- `CheckedDomain`: Two-level cache (separate allow/block `FxHashSet`s) to avoid repeated full blocklist scans
- EDNS support, Prometheus metrics emission, event sink integration

**`src/event.rs`** — Event sink system
- `Sink` trait with three implementations: `StubSink` (no-op), `S3Sink`, `DatabricksSink`
- All sinks use unbounded MPSC channels + background worker tasks for non-blocking operation
- UUIDv7 for time-ordered event IDs, newline-delimited JSON format
- `DatabricksSink` includes OAuth token caching with automatic refresh

**`src/blocklist.rs`** — Loads blocklist from file path or HTTP(S) URL. Format: one domain per line, `#` comments, trailing dot normalized.

**`src/metrics.rs`** — Prometheus metrics server (axum, `/metrics` endpoint). Counters: `dns_requests_total`, `dns_requests_block`, `dns_requests_forward`.

**`src/trace.rs`** — Optional OpenTelemetry tracing via `--otel`. Falls back to stdout logging. `OtelInitGuard` ensures proper shutdown.

**`src/main.rs`** — CLI (clap derive), component initialization, graceful shutdown via ctrl-c + cancellation tokens.

### Key Dependencies

- `hickory-server`/`hickory-client`: DNS protocol
- `tokio`: Async runtime
- `axum`: Metrics HTTP server
- `aws-sdk-s3`: S3 event sink
- `reqwest`: HTTP client (blocklist fetching, Databricks API)
- `rustc-hash`: FxHashSet for fast blocklist lookups
- `opentelemetry-otlp`: Optional distributed tracing

## Important Notes

- The blocklist is loaded once at startup into an `FxHashSet` and never reloaded
- Event sinks use unbounded channels — be mindful of memory under high query loads
- Databricks credentials support both CLI args and environment variables (`DATABRICKS_HOST`, `DATABRICKS_CLIENT_ID`, `DATABRICKS_CLIENT_SECRET`, `DATABRICKS_VOLUME_PATH`); CLI args take precedence
- The release profile enables aggressive optimization: LTO, single codegen unit, binary stripping
