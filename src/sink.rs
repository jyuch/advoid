use aws_sdk_s3::primitives::ByteStream;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct Event {
    occur: DateTime<Utc>,
    src_ip: String,
    src_port: u16,
    name: String,
    query_class: String,
    query_type: String,
    op_code: String,
}

#[async_trait::async_trait]
pub trait Sink {
    async fn send(
        &self,
        src_ip: String,
        src_port: u16,
        name: String,
        query_class: String,
        query_type: String,
        op_code: String,
    ) -> anyhow::Result<()>;
}

pub struct S3Sink {
    client: aws_sdk_s3::Client,
    bucket: String,
    prefix: Option<String>,
}

impl S3Sink {
    pub fn new(client: aws_sdk_s3::Client, bucket: String, prefix: Option<String>) -> Self {
        let prefix = prefix.map(|it| it.trim_matches(|c| c == '/').to_string());
        S3Sink {
            client,
            bucket,
            prefix,
        }
    }
}

#[async_trait::async_trait]
impl Sink for S3Sink {
    async fn send(
        &self,
        src_ip: String,
        src_port: u16,
        name: String,
        query_class: String,
        query_type: String,
        op_code: String,
    ) -> anyhow::Result<()> {
        let event = Event {
            occur: Utc::now(),
            src_ip,
            src_port,
            name,
            query_class,
            query_type,
            op_code,
        };

        let body = serde_json::to_string(&event)?;
        let body = ByteStream::from(body.into_bytes());

        let key = match self.prefix {
            Some(ref prefix) => {
                format!(
                    "{}/date={}/{}.json",
                    prefix,
                    event.occur.format("%Y-%m-%d"),
                    event.occur.format("%+")
                )
            }
            None => {
                format!(
                    "date={}/{}.json",
                    event.occur.format("%Y-%m-%d"),
                    event.occur.format("%+")
                )
            }
        };

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

pub struct StubSink {}

#[async_trait::async_trait]
impl Sink for StubSink {
    async fn send(
        &self,
        _src_ip: String,
        _src_port: u16,
        _name: String,
        _query_class: String,
        _query_type: String,
        _op_code: String,
    ) -> anyhow::Result<()> {
        Ok((/* Nothing to do. */))
    }
}
