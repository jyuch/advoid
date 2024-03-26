use clap::Parser;
use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::udp::UdpClientStream;
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::proto::op::{Header, MessageType, OpCode, ResponseCode};
use hickory_server::proto::rr::IntoName;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::ServerFuture;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::Instant;

#[derive(Parser, Debug)]
struct Cli {
    /// Bind address
    #[clap(long)]
    bind: SocketAddr,

    /// Upstream address
    #[clap(long)]
    upstream: SocketAddr,

    /// Block file path
    #[clap(long)]
    block: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();

    let mut block_list_file = File::open(opt.block).await?;
    let mut buf = String::new();
    let _ = block_list_file.read_to_string(&mut buf).await?;
    let blacklist: HashSet<String> = buf
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
    server.block_until_done().await?;

    Ok(())
}

struct StubRequestHandler {
    upstream: Arc<Mutex<AsyncClient>>,
    blacklist: HashSet<String>,
}

impl StubRequestHandler {
    pub fn new(upstream: Arc<Mutex<AsyncClient>>, blacklist: HashSet<String>) -> Self {
        StubRequestHandler {
            upstream,
            blacklist,
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for StubRequestHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        response_handle: R,
    ) -> ResponseInfo {
        let result = match request.message_type() {
            MessageType::Query => match request.op_code() {
                OpCode::Query => {
                    let upstream = &mut *self.upstream.lock().await;
                    forward_to_upstream(upstream, &self.blacklist, request, response_handle).await
                }
                _op => server_not_implement(request, response_handle).await,
            },
            MessageType::Response => server_not_implement(request, response_handle).await,
        };

        result.unwrap_or_else(|_e| {
            let mut header = Header::new();
            header.set_response_code(ResponseCode::ServFail);
            header.into()
        })
    }
}

async fn forward_to_upstream<R: ResponseHandler>(
    upstream: &mut AsyncClient,
    blacklist: &HashSet<String>,
    request: &Request,
    mut response_handle: R,
) -> anyhow::Result<ResponseInfo> {
    let name = request.query().name().into_name()?;
    let class = request.query().query_class();
    let tpe = request.query().query_type();

    let response_info = if blacklist.contains(&name.to_string()) {
        println!("{} {} {} blocking", name, class, tpe);
        let response_builder = MessageResponseBuilder::from_message_request(request);
        let response = response_builder.build(*request.header(), &[], &[], &[], &[]);
        response_handle.send_response(response).await?
    } else {
        let stopwatch = Instant::now();
        let dns_response = upstream.query(name.clone(), class, tpe).await?;
        println!(
            "{} {} {} in {}[ms]",
            name,
            class,
            tpe,
            stopwatch.elapsed().as_millis()
        );
        let response_builder = MessageResponseBuilder::from_message_request(request);
        let response =
            response_builder.build(*request.header(), dns_response.answers(), &[], &[], &[]);
        response_handle.send_response(response).await?
    };

    Ok(response_info)
}

async fn server_not_implement<R: ResponseHandler>(
    request: &Request,
    mut response_handle: R,
) -> anyhow::Result<ResponseInfo> {
    let response = MessageResponseBuilder::from_message_request(request);
    let response_info = response_handle
        .send_response(response.error_msg(request.header(), ResponseCode::NotImp))
        .await?;

    Ok(response_info)
}
