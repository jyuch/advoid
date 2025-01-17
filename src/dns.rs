use hickory_client::client::{AsyncClient, ClientHandle};
use hickory_client::op::{DnsResponse, Edns, Header, MessageType, OpCode, ResponseCode};
use hickory_client::rr::{DNSClass, IntoName, Name, Record, RecordType};
use hickory_server::authority::{MessageResponse, MessageResponseBuilder};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use rustc_hash::FxHashSet;
use std::io;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, instrument, warn};

struct CheckedDomain {
    block: FxHashSet<String>,
    allow: FxHashSet<String>,
}

impl CheckedDomain {
    pub fn new() -> Self {
        CheckedDomain {
            block: FxHashSet::default(),
            allow: FxHashSet::default(),
        }
    }
}

pub struct StubRequestHandler {
    upstream: Arc<Mutex<AsyncClient>>,
    blacklist: FxHashSet<String>,
    checked: Arc<Mutex<CheckedDomain>>,
}

impl StubRequestHandler {
    pub fn new(upstream: Arc<Mutex<AsyncClient>>, blacklist: FxHashSet<String>) -> Self {
        StubRequestHandler {
            upstream,
            blacklist,
            checked: Arc::new(Mutex::new(CheckedDomain::new())),
        }
    }

    #[instrument(skip(self))]
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

    #[instrument(skip(self))]
    async fn forward_to_upstream(
        &self,
        name: Name,
        query_class: DNSClass,
        query_type: RecordType,
    ) -> anyhow::Result<DnsResponse> {
        let mut upstream = { self.upstream.lock().await.clone() };
        let response = upstream
            .query(name.clone(), query_class, query_type)
            .await?;
        Ok(response)
    }

    #[instrument(skip_all)]
    async fn handle_query<R: ResponseHandler>(
        &self,
        response_edns: Option<Edns>,
        request: &Request,
        response_handle: R,
    ) -> anyhow::Result<ResponseInfo> {
        let name = request.query().name().into_name()?;
        let class = request.query().query_class();
        let tpe = request.query().query_type();

        let upstream_response = if self.is_blacklist_subdomain(&name.to_string()).await {
            debug!("Bypassing upstream query {}", &name.to_string());
            metrics::counter!("dns_requests_block").increment(1);
            None
        } else {
            let dns_response = self.forward_to_upstream(name.clone(), class, tpe).await?;
            metrics::counter!("dns_requests_forward").increment(1);
            Some(dns_response)
        };

        let response_builder = MessageResponseBuilder::from_message_request(request);

        let response_info = match upstream_response {
            Some(response) => {
                let mut response_header = Header::response_from_request(request.header());
                response_header.set_recursion_available(response.recursion_available());
                response_header.set_response_code(response.response_code());

                let response = response_builder.build(
                    response_header,
                    response.answers(),
                    response.name_servers(),
                    &[],
                    response.additionals(),
                );
                send_response(response_edns, response, response_handle).await?
            }
            None => {
                let response = response_builder.error_msg(request.header(), ResponseCode::NXDomain);
                send_response(response_edns, response, response_handle).await?
            }
        };

        Ok(response_info)
    }

    #[instrument(skip_all)]
    async fn server_not_implement<R: ResponseHandler>(
        &self,
        response_edns: Option<Edns>,
        request: &Request,
        response_handle: R,
    ) -> anyhow::Result<ResponseInfo> {
        let response = MessageResponseBuilder::from_message_request(request);
        let response_info = send_response(
            response_edns,
            response.error_msg(request.header(), ResponseCode::NotImp),
            response_handle,
        )
        .await?;

        Ok(response_info)
    }
}

#[async_trait::async_trait]
impl RequestHandler for StubRequestHandler {
    #[instrument(skip_all, fields(dns.src, dns.name, dns.query_class, dns.query_type, dns.op_code, dns.response_code))]
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        {
            let src = request.src().to_string();
            tracing::Span::current().record("dns.src", &src);
            let name = request.query().name().to_string();
            tracing::Span::current().record("dns.name", &name);
            let query_class = request.query().query_class().to_string();
            tracing::Span::current().record("dns.query_class", &query_class);
            let query_type = request.query().query_type().to_string();
            tracing::Span::current().record("dns.query_type", &query_type);
            let op_code = request.op_code().to_string();
            tracing::Span::current().record("dns.op_code", &op_code);
        }

        metrics::counter!("dns_requests_total").increment(1);

        // check if it's edns
        let response_edns = if let Some(req_edns) = request.edns() {
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
                OpCode::Query => {
                    self.handle_query(response_edns, request, response_handle)
                        .await
                }
                c => {
                    warn!("unimplemented op_code: {:?}", c);
                    self.server_not_implement(response_edns, request, response_handle)
                        .await
                }
            },
            MessageType::Response => {
                self.server_not_implement(response_edns, request, response_handle)
                    .await
            }
        };

        match result {
            Ok(response_info) => {
                let response_code = response_info.response_code().to_string();
                tracing::Span::current().record("dns.response_code", &response_code);
                response_info
            }
            Err(e) => {
                error!("request failed: {}", e);
                tracing::Span::current()
                    .record("dns.response_code", ResponseCode::ServFail.to_string());
                let mut header = Header::new();
                header.set_response_code(ResponseCode::ServFail);
                header.into()
            }
        }
    }
}

#[allow(unused_mut, unused_variables)]
#[instrument(skip_all)]
async fn send_response<'a, R: ResponseHandler>(
    response_edns: Option<Edns>,
    mut response: MessageResponse<
        '_,
        'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
        impl Iterator<Item = &'a Record> + Send + 'a,
    >,
    mut response_handle: R,
) -> io::Result<ResponseInfo> {
    if let Some(mut resp_edns) = response_edns {
        /*
        #[cfg(feature = "dnssec")]
        {
            // set edns DAU and DHU
            // send along the algorithms which are supported by this authority
            let mut algorithms = SupportedAlgorithms::default();
            algorithms.set(Algorithm::RSASHA256);
            algorithms.set(Algorithm::ECDSAP256SHA256);
            algorithms.set(Algorithm::ECDSAP384SHA384);
            algorithms.set(Algorithm::ED25519);

            let dau = EdnsOption::DAU(algorithms);
            let dhu = EdnsOption::DHU(algorithms);

            resp_edns.options_mut().insert(dau);
            resp_edns.options_mut().insert(dhu);
        }
        */
        response.set_edns(resp_edns);
    }

    response_handle.send_response(response).await
}
