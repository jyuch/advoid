mod dns;
mod trace;

use crate::dns::StubRequestHandler;

use axum::routing::get;
use axum::{Extension, Router};
use clap::Parser;
use hickory_client::client::AsyncClient;
use hickory_client::udp::UdpClientStream;
use hickory_server::ServerFuture;
use rustc_hash::FxHashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tower_http::add_extension::AddExtensionLayer;

#[derive(Parser, Debug)]
struct Cli {
    /// Bind address
    #[clap(long)]
    bind: SocketAddr,

    /// Upstream address
    #[clap(long)]
    upstream: SocketAddr,

    /// Console address
    #[clap(long)]
    console: SocketAddr,

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

    let counter = Arc::new(AtomicU32::new(0));

    let handler =
        StubRequestHandler::new(Arc::new(Mutex::new(upstream)), blacklist, counter.clone());
    let state = Arc::new(State::new(counter.clone()));

    let socket = UdpSocket::bind(&opt.bind).await?;
    let mut server = ServerFuture::new(handler);
    server.register_socket(socket);

    tokio::spawn(async move {
        let _ = server.block_until_done().await;
    });

    let app = Router::new()
        .route("/", get(root))
        .layer(AddExtensionLayer::new(state));

    let listener = tokio::net::TcpListener::bind(opt.console).await.unwrap();
    axum::serve(listener, app).await?;

    Ok(())
}

async fn root(Extension(state): Extension<Arc<State>>) -> String {
    let count = state.counter.load(Ordering::Relaxed);
    format!("{}", count)
}

struct State {
    counter: Arc<AtomicU32>,
}

impl State {
    pub fn new(counter: Arc<AtomicU32>) -> Self {
        Self { counter }
    }
}
