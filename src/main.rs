use clap::Parser;
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::proto::op::{Header, MessageType, OpCode, ResponseCode};
use hickory_server::proto::rr::rdata::A;
use hickory_server::proto::rr::{IntoName, RData, Record};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use hickory_server::ServerFuture;
use std::net::{SocketAddr};
use tokio::net::UdpSocket;

#[derive(Parser, Debug)]
struct Cli {
    /// Bind address
    #[clap(long)]
    bind: SocketAddr,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let opt = Cli::parse();

    let socket = UdpSocket::bind(&opt.bind).await?;
    let handler = StubRequestHandler::new();
    let mut server = ServerFuture::new(handler);
    server.register_socket(socket);
    server.block_until_done().await?;

    Ok(())
}

struct StubRequestHandler {}

impl StubRequestHandler {
    pub fn new() -> Self {
        StubRequestHandler {}
    }
}

#[async_trait::async_trait]
impl RequestHandler for StubRequestHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let result = match request.message_type() {
            MessageType::Query => match request.op_code() {
                OpCode::Query => {
                    let a = A::new(203, 0, 113, 1);
                    let rd = RData::A(a);
                    let r =
                        Record::from_rdata(request.query().name().into_name().unwrap(), 3600, rd);
                    let response = MessageResponseBuilder::from_message_request(request);
                    let response =
                        response.build(*request.header(), vec![&r], vec![], vec![], vec![]);
                    response_handle.send_response(response).await
                }
                _op => {
                    let response = MessageResponseBuilder::from_message_request(request);
                    response_handle
                        .send_response(response.error_msg(request.header(), ResponseCode::NotImp))
                        .await
                }
            },
            MessageType::Response => {
                let response = MessageResponseBuilder::from_message_request(request);
                response_handle
                    .send_response(response.error_msg(request.header(), ResponseCode::NotImp))
                    .await
            }
        };

        result.unwrap_or_else(|_e| {
            let mut header = Header::new();
            header.set_response_code(ResponseCode::ServFail);
            header.into()
        })
    }
}
