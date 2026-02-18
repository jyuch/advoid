use crate::event::Sink;
use hickory_client::client::{Client, ClientHandle};
use hickory_proto::op::{Edns, Header, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::rdata::SOA;
use hickory_proto::rr::{DNSClass, IntoName, Name, RData, Record, RecordType};
use hickory_proto::xfer::DnsResponse;
use hickory_server::authority::{MessageResponse, MessageResponseBuilder};
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use lru::LruCache;
use rustc_hash::FxHashSet;
use std::io;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, error, instrument, warn};

const RFC6303_ZONES: &[&str] = &[
    // IPv4
    "0.in-addr.arpa.",
    "127.in-addr.arpa.",
    "10.in-addr.arpa.",
    "16.172.in-addr.arpa.",
    "17.172.in-addr.arpa.",
    "18.172.in-addr.arpa.",
    "19.172.in-addr.arpa.",
    "20.172.in-addr.arpa.",
    "21.172.in-addr.arpa.",
    "22.172.in-addr.arpa.",
    "23.172.in-addr.arpa.",
    "24.172.in-addr.arpa.",
    "25.172.in-addr.arpa.",
    "26.172.in-addr.arpa.",
    "27.172.in-addr.arpa.",
    "28.172.in-addr.arpa.",
    "29.172.in-addr.arpa.",
    "30.172.in-addr.arpa.",
    "31.172.in-addr.arpa.",
    "168.192.in-addr.arpa.",
    "254.169.in-addr.arpa.",
    // IPv6 "this host" (::0/128)
    "0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa.",
    // IPv6 loopback (::1/128)
    "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa.",
    // IPv6 unique local (fd00::/8)
    "d.f.ip6.arpa.",
    // IPv6 link-local (fe80::/10)
    "8.e.f.ip6.arpa.",
    "9.e.f.ip6.arpa.",
    "a.e.f.ip6.arpa.",
    "b.e.f.ip6.arpa.",
    // IPv6 documentation (2001:db8::/32)
    "8.b.d.0.1.0.0.2.ip6.arpa.",
];

fn synthetic_soa_record_for_zone(zone: &str) -> Record {
    let mname = Name::from_ascii("ns.advoid.").unwrap();
    let rname = Name::from_ascii("hostmaster.advoid.").unwrap();
    let soa = SOA::new(
        mname, rname, 1,      // serial
        3600,   // refresh (1 hour)
        1800,   // retry (30 minutes)
        604800, // expire (1 week)
        3600,   // minimum (1 hour negative cache TTL)
    );

    let mut record =
        Record::from_rdata(Name::from_ascii(zone).unwrap(), 3600, RData::SOA(soa));
    record.set_dns_class(DNSClass::IN);
    record
}

fn synthetic_soa_record() -> Record {
    synthetic_soa_record_for_zone("advoid.")
}

fn synthetic_ns_record(zone: &str) -> Record {
    use hickory_proto::rr::rdata::NS;
    let ns_name = Name::from_ascii("ns.advoid.").unwrap();
    let mut record = Record::from_rdata(
        Name::from_ascii(zone).unwrap(),
        3600,
        RData::NS(NS(ns_name)),
    );
    record.set_dns_class(DNSClass::IN);
    record
}

fn find_rfc6303_zone(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    RFC6303_ZONES
        .iter()
        .copied()
        .filter(|zone| lower.ends_with(zone))
        .max_by_key(|zone| zone.len())
}

#[cfg(test)]
fn is_rfc6303_zone(name: &str) -> bool {
    find_rfc6303_zone(name).is_some()
}

struct CheckedDomain {
    cache: LruCache<String, bool>,
}

impl CheckedDomain {
    pub fn new(cap: NonZeroUsize) -> Self {
        CheckedDomain {
            cache: LruCache::new(cap),
        }
    }
}

pub struct StubRequestHandler {
    upstream: Arc<Mutex<Client>>,
    blacklist: FxHashSet<String>,
    checked: Arc<Mutex<CheckedDomain>>,
    sink: Arc<dyn Sink + Sync + Send>,
    block_local_zone: bool,
}

impl StubRequestHandler {
    pub fn new(
        upstream: Arc<Mutex<Client>>,
        blacklist: FxHashSet<String>,
        sink: Arc<dyn Sink + Sync + Send>,
        block_local_zone: bool,
        cache_cap: NonZeroUsize,
    ) -> Self {
        StubRequestHandler {
            upstream,
            blacklist,
            checked: Arc::new(Mutex::new(CheckedDomain::new(cache_cap))),
            sink,
            block_local_zone,
        }
    }

    #[instrument(skip(self))]
    async fn is_blacklist_subdomain(&self, domain: &String) -> bool {
        let mut checked = self.checked.lock().await;

        if let Some(&blocked) = checked.cache.get(domain) {
            metrics::gauge!("dns_cache_entries").set(checked.cache.len() as f64);
            return blocked;
        }

        for it in &self.blacklist {
            if domain == it || domain.ends_with(&format!(".{}", it)) {
                checked.cache.put(domain.to_string(), true);
                metrics::gauge!("dns_cache_entries").set(checked.cache.len() as f64);
                return true;
            }
        }

        checked.cache.put(domain.to_string(), false);
        metrics::gauge!("dns_cache_entries").set(checked.cache.len() as f64);
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
        let request_info = request.request_info()?;

        let name = request_info.query.name().into_name()?;
        let class = request_info.query.query_class();
        let tpe = request_info.query.query_type();

        let name_str = name.to_string();

        // Early return for RFC 6303 zone apex SOA/NS queries
        if self.block_local_zone {
            if let Some(zone) = find_rfc6303_zone(&name_str) {
                let is_apex = name_str.to_ascii_lowercase() == zone;
                if is_apex && (tpe == RecordType::SOA || tpe == RecordType::NS) {
                    debug!("Serving RFC 6303 zone apex {} {}", tpe, &name);
                    metrics::counter!("dns_requests_block").increment(1);

                    let response_builder = MessageResponseBuilder::from_message_request(request);
                    let mut response_header = Header::response_from_request(request.header());
                    response_header.set_response_code(ResponseCode::NoError);
                    response_header.set_authoritative(true);
                    response_header.set_recursion_available(true);

                    let answer = if tpe == RecordType::SOA {
                        synthetic_soa_record_for_zone(zone)
                    } else {
                        synthetic_ns_record(zone)
                    };
                    let answers = [answer];

                    let response = response_builder.build(
                        response_header,
                        &answers,
                        &[] as &[Record],
                        &[] as &[Record],
                        &[] as &[Record],
                    );
                    return Ok(send_response(response_edns, response, response_handle).await?);
                }
            }
        }

        let rfc6303_matched_zone = if self.block_local_zone {
            find_rfc6303_zone(&name_str)
        } else {
            None
        };

        let upstream_response = if rfc6303_matched_zone.is_some() {
            debug!("Blocking RFC 6303 local zone query {}", &name);
            metrics::counter!("dns_requests_block").increment(1);
            None
        } else if self.is_blacklist_subdomain(&name_str).await {
            debug!("Bypassing upstream query {}", &name);
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
                let soa_record = match rfc6303_matched_zone {
                    Some(zone) => synthetic_soa_record_for_zone(zone),
                    None => synthetic_soa_record(),
                };
                let soa_records = [soa_record];
                let mut response_header = Header::response_from_request(request.header());
                response_header.set_response_code(ResponseCode::NXDomain);
                response_header.set_authoritative(true);
                response_header.set_recursion_available(true);

                let response = response_builder.build(
                    response_header,
                    &[] as &[Record],
                    &[] as &[Record],
                    &soa_records,
                    &[] as &[Record],
                );
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
        match request.request_info() {
            Ok(request_info) => {
                {
                    let src = request_info.src.to_string();
                    tracing::Span::current().record("dns.src", &src);
                    let name = request_info.query.name().to_string();
                    tracing::Span::current().record("dns.name", &name);
                    let query_class = request_info.query.query_class().to_string();
                    tracing::Span::current().record("dns.query_class", &query_class);
                    let query_type = request_info.query.query_type().to_string();
                    tracing::Span::current().record("dns.query_type", &query_type);
                    let op_code = request_info.header.op_code().to_string();
                    tracing::Span::current().record("dns.op_code", &op_code);
                };

                let event_id = {
                    let src_ip = request_info.src.ip().to_string();
                    let src_port = request_info.src.port();
                    let name = request_info.query.name().to_string();
                    let query_class = request_info.query.query_class().to_string();
                    let query_type = request_info.query.query_type().to_string();
                    let op_code = request_info.header.op_code().to_string();

                    self.sink
                        .request(src_ip, src_port, name, query_class, query_type, op_code)
                        .await
                };

                metrics::counter!("dns_requests_total").increment(1);

                // check if it's edns
                let response_edns = if let Some(req_edns) = request.edns() {
                    let mut response = MessageResponseBuilder::from_message_request(request);
                    let mut response_header = Header::response_from_request(request.header());

                    let mut resp_edns: Edns = Edns::new();

                    // check our version against the request
                    // TODO: what version are we?
                    let our_version = 0;
                    resp_edns.set_dnssec_ok(req_edns.flags().dnssec_ok);
                    resp_edns.set_max_payload(1232);
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
                            let mut header = Header::response_from_request(request.header());
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
                        self.sink.response(event_id, response_code).await;
                        response_info
                    }
                    Err(e) => {
                        error!("request failed: {}", e);
                        tracing::Span::current()
                            .record("dns.response_code", ResponseCode::ServFail.to_string());
                        self.sink
                            .response(event_id, ResponseCode::ServFail.to_string())
                            .await;
                        let mut header = Header::response_from_request(request.header());
                        header.set_response_code(ResponseCode::ServFail);
                        header.into()
                    }
                }
            }
            Err(e) => {
                error!("request failed: {}", e);
                tracing::Span::current()
                    .record("dns.response_code", ResponseCode::ServFail.to_string());
                let mut header = Header::response_from_request(request.header());
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc6303_ipv4_zones() {
        // 0.0.0.0/8
        assert!(is_rfc6303_zone("1.0.0.0.in-addr.arpa."));
        // 127.0.0.0/8
        assert!(is_rfc6303_zone("1.0.0.127.in-addr.arpa."));
        // 10.0.0.0/8
        assert!(is_rfc6303_zone("1.0.0.10.in-addr.arpa."));
        // 172.16.0.0/12
        assert!(is_rfc6303_zone("1.0.16.172.in-addr.arpa."));
        assert!(is_rfc6303_zone("1.0.31.172.in-addr.arpa."));
        // 192.168.0.0/16
        assert!(is_rfc6303_zone("1.0.168.192.in-addr.arpa."));
        // 169.254.0.0/16
        assert!(is_rfc6303_zone("1.0.254.169.in-addr.arpa."));
    }

    #[test]
    fn test_rfc6303_ipv6_zones() {
        // ::1 loopback
        assert!(is_rfc6303_zone(
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.ip6.arpa."
        ));
        // fd00::/8 unique local
        assert!(is_rfc6303_zone(
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.d.f.ip6.arpa."
        ));
        // fe80::/10 link-local
        assert!(is_rfc6303_zone(
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.e.f.ip6.arpa."
        ));
        assert!(is_rfc6303_zone(
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.b.e.f.ip6.arpa."
        ));
        // 2001:db8::/32 documentation
        assert!(is_rfc6303_zone(
            "1.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.0.8.b.d.0.1.0.0.2.ip6.arpa."
        ));
    }

    #[test]
    fn test_rfc6303_non_matching() {
        assert!(!is_rfc6303_zone("example.com."));
        assert!(!is_rfc6303_zone("google.com."));
        assert!(!is_rfc6303_zone("1.0.0.8.in-addr.arpa."));
        // 172.15 is not in the private range
        assert!(!is_rfc6303_zone("1.0.15.172.in-addr.arpa."));
        // 172.32 is not in the private range
        assert!(!is_rfc6303_zone("1.0.32.172.in-addr.arpa."));
    }

    #[test]
    fn test_find_rfc6303_zone_returns_zone_name() {
        assert_eq!(
            find_rfc6303_zone("1.0.0.10.in-addr.arpa."),
            Some("10.in-addr.arpa.")
        );
        assert_eq!(
            find_rfc6303_zone("5.168.192.in-addr.arpa."),
            Some("168.192.in-addr.arpa.")
        );
    }

    #[test]
    fn test_find_rfc6303_zone_apex() {
        let zone = find_rfc6303_zone("10.in-addr.arpa.");
        assert_eq!(zone, Some("10.in-addr.arpa."));
        // Verify the name equals the zone (apex detection)
        assert_eq!("10.in-addr.arpa.", zone.unwrap());
    }

    #[test]
    fn test_find_rfc6303_zone_non_matching() {
        assert_eq!(find_rfc6303_zone("example.com."), None);
        assert_eq!(find_rfc6303_zone("google.com."), None);
        assert_eq!(find_rfc6303_zone("1.0.0.8.in-addr.arpa."), None);
    }

    #[test]
    fn test_rfc6303_case_insensitive() {
        assert!(is_rfc6303_zone("1.0.0.127.IN-ADDR.ARPA."));
        assert!(is_rfc6303_zone("1.0.0.10.In-Addr.Arpa."));
        assert!(is_rfc6303_zone("1.0.0.0.D.F.IP6.ARPA."));
    }

    async fn make_handler(blocklist: Vec<&str>) -> StubRequestHandler {
        make_handler_with_cap(blocklist, NonZeroUsize::new(10000).unwrap()).await
    }

    async fn make_handler_with_cap(
        blocklist: Vec<&str>,
        cache_cap: NonZeroUsize,
    ) -> StubRequestHandler {
        use crate::event::StubSink;
        use hickory_client::client::Client;
        use hickory_proto::runtime::TokioRuntimeProvider;
        use hickory_proto::udp::UdpClientStream;

        let blacklist: FxHashSet<String> = blocklist.into_iter().map(|s| s.to_string()).collect();
        let conn =
            UdpClientStream::builder("127.0.0.1:53".parse().unwrap(), TokioRuntimeProvider::new())
                .build();
        let (client, bg) = Client::connect(conn).await.unwrap();
        drop(bg);
        let (sink, _, _) = StubSink::new();
        StubRequestHandler::new(
            Arc::new(Mutex::new(client)),
            blacklist,
            Arc::new(sink),
            false,
            cache_cap,
        )
    }

    #[tokio::test]
    async fn test_blocklist_exact_match() {
        let handler = make_handler(vec!["ad.com."]).await;
        assert!(handler.is_blacklist_subdomain(&"ad.com.".to_string()).await);
    }

    #[tokio::test]
    async fn test_blocklist_subdomain_match() {
        let handler = make_handler(vec!["ad.com."]).await;
        assert!(
            handler
                .is_blacklist_subdomain(&"sub.ad.com.".to_string())
                .await
        );
        assert!(
            handler
                .is_blacklist_subdomain(&"deep.sub.ad.com.".to_string())
                .await
        );
    }

    #[tokio::test]
    async fn test_blocklist_no_label_boundary_false_positive() {
        let handler = make_handler(vec!["ad.com."]).await;
        assert!(
            !handler
                .is_blacklist_subdomain(&"bad.com.".to_string())
                .await
        );
        assert!(
            !handler
                .is_blacklist_subdomain(&"mad.com.".to_string())
                .await
        );
    }

    #[tokio::test]
    async fn test_blocklist_allowed_domain_not_blocked() {
        let handler = make_handler(vec!["example.com."]).await;
        assert!(
            !handler
                .is_blacklist_subdomain(&"notexample.com.".to_string())
                .await
        );
    }

    #[tokio::test]
    async fn test_cache_eviction_returns_correct_results() {
        // Cache capacity of 2: after inserting 3 entries, the oldest is evicted
        let handler =
            make_handler_with_cap(vec!["ad.com."], NonZeroUsize::new(2).unwrap()).await;

        // Fill cache with 2 entries
        assert!(handler.is_blacklist_subdomain(&"ad.com.".to_string()).await);
        assert!(
            !handler
                .is_blacklist_subdomain(&"good.com.".to_string())
                .await
        );

        // Insert a 3rd entry, evicting "ad.com." from the cache
        assert!(
            !handler
                .is_blacklist_subdomain(&"other.com.".to_string())
                .await
        );

        // "ad.com." was evicted but should still be correctly identified as blocked
        assert!(handler.is_blacklist_subdomain(&"ad.com.".to_string()).await);
    }
}
