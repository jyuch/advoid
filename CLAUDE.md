# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

advoid is a DNS-based ad blocker written in Rust. It acts as a DNS stub resolver that intercepts queries, checks them against a blocklist, and returns NXDOMAIN for blocked domains while forwarding allowed queries to an upstream DNS resolver.

## Build and Development Commands

### Building
```bash
# Debug build
cargo build

# Release build (with LTO and stripping enabled)
cargo build --release
```

### Running
```bash
# Run with minimum required arguments
cargo run -- --bind 127.0.0.1:53 --upstream 1.1.1.1:53 --exporter 127.0.0.1:3000 --block path/to/blocklist.txt

# Run with S3 event sink
cargo run -- --bind 127.0.0.1:53 --upstream 1.1.1.1:53 --exporter 127.0.0.1:3000 --block path/to/blocklist.txt --sink s3 --s3-bucket my-bucket --s3-prefix logs

# Run with Databricks event sink
cargo run -- --bind 127.0.0.1:53 --upstream 1.1.1.1:53 --exporter 127.0.0.1:3000 --block path/to/blocklist.txt --sink databricks --databricks-host https://workspace.cloud.databricks.com --databricks-client-id <client-id> --databricks-client-secret <secret> --databricks-volume-path /Volumes/catalog/schema/volume

# Run with Databricks sink using environment variables (recommended for security)
export DATABRICKS_HOST=https://workspace.cloud.databricks.com
export DATABRICKS_CLIENT_ID=<client-id>
export DATABRICKS_CLIENT_SECRET=<secret>
export DATABRICKS_VOLUME_PATH=/Volumes/catalog/schema/volume
cargo run -- --bind 127.0.0.1:53 --upstream 1.1.1.1:53 --exporter 127.0.0.1:3000 --block path/to/blocklist.txt --sink databricks

# Run with OpenTelemetry tracing
cargo run -- --bind 127.0.0.1:53 --upstream 1.1.1.1:53 --exporter 127.0.0.1:3000 --block path/to/blocklist.txt --otel http://localhost:4317
```

### Docker
```bash
# Build Docker image
docker build -t advoid .

# Run container
docker run -p 53:53/udp -p 3000:3000 advoid --bind 0.0.0.0:53 --upstream 1.1.1.1:53 --exporter 0.0.0.0:3000 --block /path/to/blocklist.txt

# Run with Databricks sink using environment variables
docker run \
  -e DATABRICKS_HOST=https://workspace.cloud.databricks.com \
  -e DATABRICKS_CLIENT_ID=<client-id> \
  -e DATABRICKS_CLIENT_SECRET=<secret> \
  -e DATABRICKS_VOLUME_PATH=/Volumes/catalog/schema/volume \
  -p 53:53/udp -p 3000:3000 \
  advoid --bind 0.0.0.0:53 --upstream 1.1.1.1:53 --exporter 0.0.0.0:3000 --block /path/to/blocklist.txt --sink databricks
```

### Testing
The project uses Rust edition 2024. Run tests with:
```bash
cargo test
```

## Architecture

### Core Components

**DNS Request Handler (`src/dns.rs`)**
- `StubRequestHandler`: Main request handler implementing hickory-server's `RequestHandler` trait
- Checks queries against blocklist using domain suffix matching
- Forwards allowed queries to upstream resolver
- Caches checked domains in memory (separate allow/block sets) to avoid repeated blocklist lookups
- Supports EDNS (Extension Mechanisms for DNS)
- Emits Prometheus metrics and trace events for all requests

**Blocklist Management (`src/blocklist.rs`)**
- `get()` function loads blocklist from file path or HTTP(S) URL
- Blocklist format: one domain per line, # for comments, blank lines ignored
- Domains are normalized with trailing dot for suffix matching

**Event Sink System (`src/event.rs`)**
- `Sink` trait: Asynchronous interface for logging DNS request/response events
- `Request`/`Response` structs: Event data models with UUIDv7 IDs and timestamps
- Three implementations:
  - `StubSink`: No-op sink for when event logging is disabled
  - `S3Sink`: Batches events and uploads as newline-delimited JSON to S3
  - `DatabricksSink`: Batches events and uploads to Databricks volumes via Files API with OAuth token caching
- All sinks use unbounded channels and background workers for non-blocking operation
- Batch size and interval are configurable via CLI flags

**Metrics (`src/metrics.rs`)**
- Prometheus metrics server using axum
- Exposes `/metrics` endpoint
- Tracks: `dns_requests_total`, `dns_requests_block`, `dns_requests_forward`

**Tracing (`src/trace.rs`)**
- Optional OpenTelemetry integration for traces and metrics
- Falls back to stdout-only logging when OTel is not configured
- `OtelInitGuard` ensures proper shutdown of trace provider

### Data Flow

1. DNS query arrives at UDP socket (bound via `--bind`)
2. `StubRequestHandler` checks domain against blocklist (cached + suffix match)
3. If blocked: Return NXDOMAIN, emit block metric
4. If allowed: Forward to upstream resolver, return response, emit forward metric
5. Request/response events are sent to configured sink (S3/Databricks/stub)
6. Background workers batch and upload events periodically

### Key Dependencies

- `hickory-server`/`hickory-client`: DNS protocol implementation
- `tokio`: Async runtime
- `axum`: Metrics HTTP server
- `aws-sdk-s3`: S3 event sink
- `reqwest`: HTTP client for blocklist fetching and Databricks API
- `opentelemetry-otlp`: Optional distributed tracing
- `rustc-hash`: Fast hash implementation (FxHashSet) for blocklist

## Important Notes

- The blocklist is loaded once at startup and stored in memory (FxHashSet)
- Checked domains are cached in a separate cache to avoid repeated full blocklist scans
- Event sinks use unbounded channels - be mindful of memory usage under high query loads
- The S3 and Databricks sinks create separate worker tasks for request and response events
- UUIDv7 is used for event IDs to maintain time-ordering
- The release profile enables aggressive optimization: LTO, single codegen unit, and binary stripping

### Credential Management

Databricks credentials can be provided via command-line arguments or environment variables:
- `DATABRICKS_HOST` - Workspace URL
- `DATABRICKS_CLIENT_ID` - Service principal client ID
- `DATABRICKS_CLIENT_SECRET` - Service principal client secret
- `DATABRICKS_VOLUME_PATH` - Volume path for event storage

**Security Best Practice**: Use environment variables for credentials instead of CLI arguments. CLI arguments are visible in process lists and shell history, while environment variables provide better security isolation.

Command-line arguments take precedence over environment variables when both are provided.
