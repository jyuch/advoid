use clap::Parser;
use hickory_client::client::{AsyncClient, ClientHandle};

use hickory_client::udp::UdpClientStream;
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::proto::op::{Header, MessageType, OpCode, ResponseCode};

use hickory_server::proto::rr::{IntoName};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
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
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();

    let conn = UdpClientStream::<UdpSocket>::new(opt.upstream);
    let (upstream, background) = AsyncClient::connect(conn).await?;
    let _handle = tokio::spawn(background);
    let handler = StubRequestHandler::new(Arc::new(Mutex::new(upstream)));

    let socket = UdpSocket::bind(&opt.bind).await?;
    let mut server = ServerFuture::new(handler);
    server.register_socket(socket);
    server.block_until_done().await?;

    Ok(())
}

struct StubRequestHandler {
    upstream: Arc<Mutex<AsyncClient>>,
}

impl StubRequestHandler {
    pub fn new(upstream: Arc<Mutex<AsyncClient>>) -> Self {
        StubRequestHandler { upstream }
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
                    forward_to_upstream(upstream, request, response_handle).await
                }
                _op => {
                    server_not_implement(request, response_handle).await
                }
            },
            MessageType::Response => {
                server_not_implement(request, response_handle).await
            }
        };

        result.unwrap_or_else(|_e| {
            let mut header = Header::new();
            header.set_response_code(ResponseCode::ServFail);
            header.into()
        })
    }
}

async fn forward_to_upstream<R: ResponseHandler>(upstream: &mut AsyncClient, request: &Request, mut response_handle: R) -> anyhow::Result<ResponseInfo> {
    let response = upstream
        .query(
            request.query().name().into_name().unwrap(),
            request.query().query_class(),
            request.query().query_type(),
        )
        .await?;

    let response_builder = MessageResponseBuilder::from_message_request(request);
    let response = response_builder.build(
        *request.header(),
        response.answers(),
        vec![],
        vec![],
        vec![],
    );
    let response_info = response_handle.send_response(response).await?;

    Ok(response_info)
}

async fn server_not_implement<R: ResponseHandler>(request: &Request, mut response_handle: R) -> anyhow::Result<ResponseInfo> {
    let response = MessageResponseBuilder::from_message_request(request);
    let response_info = response_handle
        .send_response(response.error_msg(request.header(), ResponseCode::NotImp))
        .await?;

    Ok(response_info)
}
