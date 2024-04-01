use clap::Parser;
use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::udp::UdpClientStream;
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::proto::op::{Edns, Header, MessageType, OpCode, ResponseCode};
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
use tracing::{error, warn};
use tracing_subscriber::fmt::time::LocalTime;
use tracing_subscriber::EnvFilter;

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
    tracing_subscriber::fmt()
        .with_timer(LocalTime::rfc_3339())
        .with_env_filter(EnvFilter::from_default_env())
        //.with_file(true)
        //.with_line_number(true)
        .init();

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

struct CheckedDomain {
    block: HashSet<String>,
    allow: HashSet<String>,
}

impl CheckedDomain {
    pub fn new() -> Self {
        CheckedDomain {
            block: HashSet::new(),
            allow: HashSet::new(),
        }
    }
}

struct StubRequestHandler {
    upstream: Arc<Mutex<AsyncClient>>,
    blacklist: HashSet<String>,
    checked: Arc<Mutex<CheckedDomain>>,
}

impl StubRequestHandler {
    pub fn new(upstream: Arc<Mutex<AsyncClient>>, blacklist: HashSet<String>) -> Self {
        StubRequestHandler {
            upstream,
            blacklist,
            checked: Arc::new(Mutex::new(CheckedDomain::new())),
        }
    }

    async fn is_blacklist_subdomain(&self, domain: &String) -> bool {
        let mut checked = self.checked.lock().await;

        if checked.block.contains(domain) {
            return true;
        }

        if checked.allow.contains(domain) {
            return false;
        }

        for it in &self.blacklist {
            if domain.ends_with(it) {
                checked.block.insert(domain.to_string());
                return true;
            }
        }

        checked.allow.insert(domain.to_string());
        false
    }

    async fn forward_to_upstream<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> anyhow::Result<ResponseInfo> {
        let name = request.query().name().into_name()?;
        let class = request.query().query_class();
        let tpe = request.query().query_type();

        let response_info = if self.is_blacklist_subdomain(&name.to_string()).await {
            let response_builder = MessageResponseBuilder::from_message_request(request);
            let response = response_builder.build(*request.header(), &[], &[], &[], &[]);
            response_handle.send_response(response).await?
        } else {
            let dns_response = self
                .upstream
                .lock()
                .await
                .query(name.clone(), class, tpe)
                .await?;

            let response_builder = MessageResponseBuilder::from_message_request(request);
            let response =
                response_builder.build(*request.header(), dns_response.answers(), &[], &[], &[]);
            response_handle.send_response(response).await?
        };

        Ok(response_info)
    }

    async fn server_not_implement<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> anyhow::Result<ResponseInfo> {
        let response = MessageResponseBuilder::from_message_request(request);
        let response_info = response_handle
            .send_response(response.error_msg(request.header(), ResponseCode::NotImp))
            .await?;

        Ok(response_info)
    }
}

#[async_trait::async_trait]
impl RequestHandler for StubRequestHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        // check if it's edns
        let _response_edns = if let Some(req_edns) = request.edns() {
            let mut response = MessageResponseBuilder::from_message_request(request);
            let mut response_header = Header::response_from_request(request.header());

            let mut resp_edns: Edns = Edns::new();

            // check our version against the request
            // TODO: what version are we?
            let our_version = 0;
            resp_edns.set_dnssec_ok(true);
            resp_edns.set_max_payload(req_edns.max_payload().max(512));
            resp_edns.set_version(our_version);

            if req_edns.version() > our_version {
                warn!(
                    "request edns version greater than {}: {}",
                    our_version,
                    req_edns.version()
                );
                response_header.set_response_code(ResponseCode::BADVERS);
                resp_edns.set_rcode_high(ResponseCode::BADVERS.high());
                response.edns(resp_edns);

                // TODO: should ResponseHandle consume self?
                let result = response_handle
                    .send_response(response.build_no_records(response_header))
                    .await;

                // couldn't handle the request
                return result.unwrap_or_else(|e| {
                    error!("request error: {}", e);
                    let mut header = Header::new();
                    header.set_response_code(ResponseCode::ServFail);
                    header.into()
                });
            }

            Some(resp_edns)
        } else {
            None
        };

        let result = match request.message_type() {
            MessageType::Query => match request.op_code() {
                OpCode::Query => self.forward_to_upstream(request, response_handle).await,
                c => {
                    warn!("unimplemented op_code: {:?}", c);
                    self.server_not_implement(request, response_handle).await
                }
            },
            MessageType::Response => self.server_not_implement(request, response_handle).await,
        };

        result.unwrap_or_else(|e| {
            error!("request failed: {}", e);
            let mut header = Header::new();
            header.set_response_code(ResponseCode::ServFail);
            header.into()
        })
    }
}
