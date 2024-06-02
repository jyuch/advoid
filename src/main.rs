use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use clap::Parser;
use hickory_client::client::AsyncClient;
use hickory_client::udp::UdpClientStream;
use hickory_server::ServerFuture;
use rustc_hash::FxHashSet;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;

use crate::dns::StubRequestHandler;

mod dns;
mod metrics;
mod trace;

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

    /// Block file path
    #[clap(long)]
    block: PathBuf,

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
        Some(trace::init_tracing(service, version, otel))
    } else {
        trace::init_tracing_without_otel();
        None
    };

    let mut block_list_file = File::open(opt.block).await?;
    let mut buf = String::new();
    let _ = block_list_file.read_to_string(&mut buf).await?;
    let blacklist: FxHashSet<String> = buf
        .lines()
        .map(|it| it.trim().to_string())
        .filter(|it| !it.is_empty())
        .filter(|it| !it.starts_with('#'))
        .map(|it| format!("{}.", it))
        .collect();

    let conn = UdpClientStream::<UdpSocket>::new(opt.upstream);
    let (upstream, background) = AsyncClient::connect(conn).await?;
    let _handle = tokio::spawn(background);

    let handler = StubRequestHandler::new(Arc::new(Mutex::new(upstream)), blacklist);

    let socket = UdpSocket::bind(&opt.bind).await?;
    let mut server = ServerFuture::new(handler);
    server.register_socket(socket);

    tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });

    metrics::start_metrics_server(opt.exporter).await?;

    Ok(())
}
