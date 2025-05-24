use advoid::dns::StubRequestHandler;
use clap::Parser;
use hickory_client::client::Client;
use hickory_proto::runtime::TokioRuntimeProvider;
use hickory_proto::udp::UdpClientStream;
use hickory_server::ServerFuture;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

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

    let blocklist = advoid::blocklist::get(opt.block).await?;

    let conn = UdpClientStream::builder(opt.upstream, TokioRuntimeProvider::new()).build();
    let (upstream, background) = Client::connect(conn).await?;
    let _handle = tokio::spawn(background);

    let handler = StubRequestHandler::new(Arc::new(Mutex::new(upstream)), blocklist);

    let socket = UdpSocket::bind(&opt.bind).await?;
    let mut server = ServerFuture::new(handler);
    server.register_socket(socket);

    tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });

    advoid::metrics::start_metrics_server(opt.exporter).await?;

    Ok(())
}
