use advoid::dns::StubRequestHandler;
use advoid::event::{DatabricksSink, S3Sink, Sink, StubSink};
use aws_config::BehaviorVersion;
use clap::{Parser, ValueEnum};
use hickory_client::client::Client;
use hickory_proto::runtime::TokioRuntimeProvider;
use hickory_proto::udp::UdpClientStream;
use hickory_server::ServerFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::signal;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::error;

#[derive(ValueEnum, Debug, Clone)]
enum SinkMode {
    S3,
    Databricks,
}

#[derive(Parser, Debug)]
struct Cli {
    /// Bind address
    #[clap(long)]
    bind: SocketAddr,

    /// Upstream address
    #[clap(long)]
    upstream: SocketAddr,

    /// Prometheus exporter endpoint
    #[clap(long)]
    exporter: SocketAddr,

    /// Block file path or url
    #[clap(long)]
    block: String,

    /// OTel endpoint
    #[clap(long)]
    otel: Option<String>,

    /// Sink mode
    #[clap(long)]
    sink: Option<SinkMode>,

    /// S3 bucket
    #[clap(long, required_if_eq("sink", "s3"))]
    s3_bucket: Option<String>,

    /// S3 prefix
    #[clap(long)]
    s3_prefix: Option<String>,

    /// Databricks workspace URL
    #[clap(long, required_if_eq("sink", "databricks"))]
    databricks_host: Option<String>,

    /// Databricks service principal client ID
    #[clap(long, required_if_eq("sink", "databricks"))]
    databricks_client_id: Option<String>,

    /// Databricks service principal client secret
    #[clap(long, required_if_eq("sink", "databricks"))]
    databricks_client_secret: Option<String>,

    /// Databricks volume path (e.g., /Volumes/catalog/schema/volume_name)
    #[clap(long, required_if_eq("sink", "databricks"))]
    databricks_volume_path: Option<String>,

    /// Event sink interval
    #[clap(long, default_value_t = 1)]
    sink_interval: u64,

    /// Event sink batch size
    #[clap(long, default_value_t = 1000)]
    sink_batch_size: usize,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();

    let _otel = if let Some(otel) = opt.otel {
        let service = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        Some(advoid::trace::init_tracing(service, version, otel))
    } else {
        advoid::trace::init_tracing_without_otel();
        None
    };

    let worker_cancellation_token = CancellationToken::new();

    let (sink, _request_worker_handle, _response_worker_handle): (
        Arc<dyn Sink + Sync + Send>,
        _,
        _,
    ) = match opt.sink {
        Some(SinkMode::S3) => {
            let config = aws_config::load_defaults(BehaviorVersion::latest()).await;
            let client = aws_sdk_s3::Client::new(&config);
            let (sink, request_worker, response_worker) = S3Sink::new(
                client,
                opt.s3_bucket.unwrap(/* Guard by clap required_if_eq */),
                opt.s3_prefix,
                opt.sink_interval,
                opt.sink_batch_size,
                worker_cancellation_token.clone(),
            );
            (
                Arc::new(sink),
                tokio::spawn(request_worker),
                tokio::spawn(response_worker),
            )
        }
        Some(SinkMode::Databricks) => {
            let (sink, request_worker, response_worker) = DatabricksSink::new(
                opt.databricks_host.unwrap(/* Guard by clap required_if_eq */),
                opt.databricks_client_id.unwrap(/* Guard by clap required_if_eq */),
                opt.databricks_client_secret.unwrap(/* Guard by clap required_if_eq */),
                opt.databricks_volume_path.unwrap(/* Guard by clap required_if_eq */),
                opt.sink_interval,
                opt.sink_batch_size,
                worker_cancellation_token.clone(),
            );
            (
                Arc::new(sink),
                tokio::spawn(request_worker),
                tokio::spawn(response_worker),
            )
        }
        None => {
            let (sink, request_worker, response_worker) = StubSink::new();
            (
                Arc::new(sink),
                tokio::spawn(request_worker),
                tokio::spawn(response_worker),
            )
        }
    };

    let blocklist = advoid::blocklist::get(opt.block).await?;

    let conn = UdpClientStream::builder(opt.upstream, TokioRuntimeProvider::new()).build();
    let (upstream, background) = Client::connect(conn).await?;
    let _handle = tokio::spawn(background);

    let handler = StubRequestHandler::new(Arc::new(Mutex::new(upstream)), blocklist, sink);

    let socket = UdpSocket::bind(&opt.bind).await?;
    let mut server = ServerFuture::new(handler);
    server.register_socket(socket);
    let server_cancellation_token = server.shutdown_token().clone();

    let server_handle = tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });

    tokio::spawn(async move {
        let _ = advoid::metrics::start_metrics_server(opt.exporter).await;
    });

    match signal::ctrl_c().await {
        Ok(()) => {}
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    server_cancellation_token.cancel();
    let _ = server_handle.await;

    worker_cancellation_token.cancel();
    let _ = _request_worker_handle.await;
    let _ = _response_worker_handle.await;

    Ok(())
}
