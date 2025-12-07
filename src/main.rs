use advoid::dns::StubRequestHandler;
use advoid::event::{S3Sink, Sink, StubSink};
use aws_config::BehaviorVersion;
use clap::{Parser, ValueEnum};
use hickory_client::client::Client;
use hickory_proto::runtime::TokioRuntimeProvider;
use hickory_proto::udp::UdpClientStream;
use hickory_server::ServerFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

#[derive(ValueEnum, Debug, Clone)]
enum SinkMode {
    S3,
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();

    let _guard = if let Some(otel) = opt.otel {
        let service = env!("CARGO_PKG_NAME");
        let version = env!("CARGO_PKG_VERSION");
        Some(advoid::trace::init_tracing(service, version, otel))
    } else {
        advoid::trace::init_tracing_without_otel();
        None
    };

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

    tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });

    advoid::metrics::start_metrics_server(opt.exporter).await?;

    Ok(())
}
