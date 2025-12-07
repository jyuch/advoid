use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::time::Duration;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::time::sleep;
use tracing::error;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
pub struct Request {
    id: Uuid,
    occur: DateTime<Utc>,
    src_ip: String,
    src_port: u16,
    name: String,
    query_class: String,
    query_type: String,
    op_code: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    id: Uuid,
    occur: DateTime<Utc>,
    response_code: String,
}

#[async_trait::async_trait]
pub trait Sink {
    async fn request(
        &self,
        src_ip: String,
        src_port: u16,
        name: String,
        query_class: String,
        query_type: String,
        op_code: String,
    ) -> Uuid;

    async fn response(&self, id: Uuid, response_code: String);
}

pub struct S3Sink {
    request_tx: UnboundedSender<Request>,
    response_tx: UnboundedSender<Response>,
}

impl S3Sink {
    pub fn new(
        client: Client,
        bucket: String,
        prefix: Option<String>,
    ) -> (S3Sink, impl Future<Output = ()>, impl Future<Output = ()>) {
        let (request_tx, request_worker) =
            initialize_worker::<Request>(client.clone(), bucket.clone(), prefix.clone(), "request");
        let (response_tx, response_worker) =
            initialize_worker::<Response>(client, bucket, prefix, "response");

        let sink = S3Sink {
            request_tx,
            response_tx,
        };

        (sink, request_worker, response_worker)
    }
}

#[async_trait::async_trait]
impl Sink for S3Sink {
    async fn request(
        &self,
        src_ip: String,
        src_port: u16,
        name: String,
        query_class: String,
        query_type: String,
        op_code: String,
    ) -> Uuid {
        let id = Uuid::now_v7();

        let event = Request {
            id,
            occur: Utc::now(),
            src_ip,
            src_port,
            name,
            query_class,
            query_type,
            op_code,
        };

        let result = self.request_tx.send(event);

        if let Err(e) = result {
            error!("error sending event: {:?}", e);
        }

        id
    }

    async fn response(&self, id: Uuid, response_code: String) {
        let event = Response {
            id,
            occur: Utc::now(),
            response_code,
        };

        let result = self.response_tx.send(event);

        if let Err(e) = result {
            error!("error sending response: {:?}", e);
        }
    }
}

pub struct StubSink {}

impl StubSink {
    pub fn new() -> (StubSink, impl Future<Output = ()>, impl Future<Output = ()>) {
        (StubSink {}, async {}, async {})
    }
}

#[async_trait::async_trait]
impl Sink for StubSink {
    async fn request(
        &self,
        _src_ip: String,
        _src_port: u16,
        _name: String,
        _query_class: String,
        _query_type: String,
        _op_code: String,
    ) -> Uuid {
        Uuid::default(/* Nothing to do. */)
    }

    async fn response(&self, _id: Uuid, _response_code: String) {
        /* Nothing to do. */
    }
}

fn key(prefix: Option<&String>, occur: DateTime<Utc>, event_type: &str) -> String {
    match prefix {
        Some(prefix) => {
            format!(
                "{}/{}/{}/{}.json",
                prefix,
                event_type,
                occur.format("%Y-%m-%d"),
                occur.format("%Y%m%d%H%M%S")
            )
        }
        None => {
            format!(
                "{}/{}/{}.json",
                event_type,
                occur.format("%Y-%m-%d"),
                occur.format("%Y%m%d%H%M%S")
            )
        }
    }
}

fn initialize_worker<T>(
    client: Client,
    bucket: String,
    prefix: Option<String>,
    event_type: &str,
) -> (UnboundedSender<T>, impl Future<Output = ()>)
where
    T: Serialize,
{
    let (tx, mut rx) = unbounded_channel();
    let worker = async move {
        let mut event_buffer = Vec::with_capacity(1000);

        while rx.recv_many(&mut event_buffer, 1000).await != 0 {
            let mut json_buffer = Vec::new();

            for it in &event_buffer {
                match serde_json::to_string(it) {
                    Ok(json) => {
                        let _ = json_buffer.write(json.as_ref());
                    }
                    Err(e) => {
                        error!("Error serializing event to JSON: {:?}", e);
                    }
                }
            }

            let key = key(prefix.as_ref(), Utc::now(), event_type);
            let body = ByteStream::from(json_buffer);

            let result = client
                .put_object()
                .bucket(bucket.clone())
                .key(key)
                .body(body)
                .send()
                .await;

            if let Err(e) = result {
                error!("error sending request event: {:?}", e);
            }

            event_buffer.clear();
            sleep(Duration::from_secs(1)).await;
        }
    };

    (tx, worker)
}
