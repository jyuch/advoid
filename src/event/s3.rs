use super::Sink;
use super::channel::ChannelSink;
use super::worker::{EventUploader, initialize_worker};
use super::{Request, Response};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use chrono::{DateTime, Utc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

struct S3Uploader {
    client: Client,
    bucket: String,
    prefix: Option<String>,
}

#[async_trait::async_trait]
impl EventUploader for S3Uploader {
    async fn upload(&self, event_type: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let key = s3_key(self.prefix.as_ref(), Utc::now(), event_type);
        let body = ByteStream::from(data);

        self.client
            .put_object()
            .bucket(self.bucket.clone())
            .key(key)
            .body(body)
            .send()
            .await?;

        Ok(())
    }
}

fn s3_key(prefix: Option<&String>, occur: DateTime<Utc>, event_type: &str) -> String {
    let id = Uuid::now_v7();
    match prefix {
        Some(prefix) => {
            format!(
                "{}/{}/{}/{}.json",
                prefix,
                event_type,
                occur.format("%Y-%m-%d"),
                id,
            )
        }
        None => {
            format!("{}/{}/{}.json", event_type, occur.format("%Y-%m-%d"), id,)
        }
    }
}

pub struct S3Sink {
    inner: ChannelSink,
}

impl S3Sink {
    pub fn new(
        client: Client,
        bucket: String,
        prefix: Option<String>,
        sink_interval: u64,
        sink_batch_size: usize,
        cancellation_token: CancellationToken,
    ) -> (S3Sink, impl Future<Output = ()>, impl Future<Output = ()>) {
        let request_uploader = S3Uploader {
            client: client.clone(),
            bucket: bucket.clone(),
            prefix: prefix.clone(),
        };
        let response_uploader = S3Uploader {
            client,
            bucket,
            prefix,
        };

        let (request_tx, request_worker) = initialize_worker::<Request>(
            request_uploader,
            "request",
            sink_interval,
            sink_batch_size,
            cancellation_token.clone(),
        );
        let (response_tx, response_worker) = initialize_worker::<Response>(
            response_uploader,
            "response",
            sink_interval,
            sink_batch_size,
            cancellation_token,
        );

        let sink = S3Sink {
            inner: ChannelSink::new(request_tx, response_tx),
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
        self.inner
            .request(src_ip, src_port, name, query_class, query_type, op_code)
            .await
    }

    async fn response(&self, id: Uuid, response_code: String) {
        self.inner.response(id, response_code).await
    }
}
