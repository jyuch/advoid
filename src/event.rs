use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;
use tokio::select;
use tokio::sync::RwLock;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;
use uuid::Uuid;

const NEWLINE: &str = "\n";

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
        sink_interval: u64,
        sink_batch_size: usize,
        cancellation_token: CancellationToken,
    ) -> (S3Sink, impl Future<Output = ()>, impl Future<Output = ()>) {
        let (request_tx, request_worker) = initialize_s3_worker::<Request>(
            client.clone(),
            bucket.clone(),
            prefix.clone(),
            "request",
            sink_interval,
            sink_batch_size,
            cancellation_token.clone(),
        );
        let (response_tx, response_worker) = initialize_s3_worker::<Response>(
            client,
            bucket,
            prefix,
            "response",
            sink_interval,
            sink_batch_size,
            cancellation_token.clone(),
        );

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

fn initialize_s3_worker<T>(
    client: Client,
    bucket: String,
    prefix: Option<String>,
    event_type: &str,
    sink_interval: u64,
    sink_batch_size: usize,
    cancellation_token: CancellationToken,
) -> (UnboundedSender<T>, impl Future<Output = ()>)
where
    T: Serialize,
{
    let (tx, mut rx) = unbounded_channel();
    let worker = async move {
        let mut event_buffer = Vec::with_capacity(sink_batch_size);

        while rx.recv_many(&mut event_buffer, sink_batch_size).await != 0 {
            let mut json_buffer = Vec::new();

            for it in &event_buffer {
                match serde_json::to_string(it) {
                    Ok(json) => {
                        let _ = json_buffer.write(json.as_ref());
                        let _ = json_buffer.write(NEWLINE.as_ref());
                    }
                    Err(e) => {
                        error!("Error serializing event to JSON: {:?}", e);
                    }
                }
            }

            let key = s3_key(prefix.as_ref(), Utc::now(), event_type);
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

            select! {
                _ = sleep(Duration::from_secs(sink_interval)) => {  },
                _ = cancellation_token.cancelled() => {  }
            }

            if rx.is_empty() && cancellation_token.is_cancelled() {
                break;
            }
        }
    };

    (tx, worker)
}

// Databricks Sink

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

struct TokenInfo {
    access_token: String,
    expires_at: DateTime<Utc>,
}

struct DatabricksClient {
    http_client: reqwest::Client,
    host: String,
    client_id: String,
    client_secret: String,
    volume_path: String,
    token: RwLock<Option<TokenInfo>>,
}

impl DatabricksClient {
    fn new(host: String, client_id: String, client_secret: String, volume_path: String) -> Self {
        Self {
            http_client: reqwest::Client::new(),
            host,
            client_id,
            client_secret,
            volume_path,
            token: RwLock::new(None),
        }
    }

    async fn get_token(&self) -> anyhow::Result<String> {
        // Check cached token
        {
            let token_guard = self.token.read().await;
            if let Some(token_info) = token_guard.as_ref() {
                // Return cached token if it's valid for at least 60 more seconds
                if token_info.expires_at > Utc::now() + chrono::Duration::seconds(60) {
                    return Ok(token_info.access_token.clone());
                }
            }
        }

        // Token is expired or doesn't exist, acquire new one
        let mut token_guard = self.token.write().await;

        // Double-check after acquiring write lock
        if let Some(token_info) = token_guard.as_ref()
            && token_info.expires_at > Utc::now() + chrono::Duration::seconds(60)
        {
            return Ok(token_info.access_token.clone());
        }

        // Fetch new token
        let token_url = format!("{}/oidc/v1/token", self.host);
        let params = [
            ("grant_type", "client_credentials"),
            ("client_id", &self.client_id),
            ("client_secret", &self.client_secret),
            ("scope", "all-apis"),
        ];

        let response = self
            .http_client
            .post(&token_url)
            .form(&params)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to get OAuth token: {} - {}", status, body);
        }

        let token_response: TokenResponse = response.json().await?;

        let expires_at = Utc::now() + chrono::Duration::seconds(token_response.expires_in as i64);
        let access_token = token_response.access_token.clone();

        *token_guard = Some(TokenInfo {
            access_token: token_response.access_token,
            expires_at,
        });

        Ok(access_token)
    }

    async fn put_file(&self, path: &str, body: Vec<u8>) -> anyhow::Result<()> {
        let token = self.get_token().await?;

        let url = format!(
            "{}/api/2.0/fs/files{}/{}",
            self.host, self.volume_path, path
        );

        let response = self
            .http_client
            .put(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/octet-stream")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Failed to upload file to Databricks: {} - {}", status, body);
        }

        Ok(())
    }
}

fn databricks_path(occur: DateTime<Utc>, event_type: &str) -> String {
    let id = Uuid::now_v7();
    format!("{}/{}/{}.json", event_type, occur.format("%Y-%m-%d"), id)
}

pub struct DatabricksSink {
    request_tx: UnboundedSender<Request>,
    response_tx: UnboundedSender<Response>,
}

impl DatabricksSink {
    pub fn new(
        host: String,
        client_id: String,
        client_secret: String,
        volume_path: String,
        sink_interval: u64,
        sink_batch_size: usize,
        cancellation_token: CancellationToken,
    ) -> (
        DatabricksSink,
        impl Future<Output = ()>,
        impl Future<Output = ()>,
    ) {
        let client = Arc::new(DatabricksClient::new(
            host,
            client_id,
            client_secret,
            volume_path,
        ));

        let (request_tx, request_worker) = initialize_databricks_worker::<Request>(
            client.clone(),
            "request",
            sink_interval,
            sink_batch_size,
            cancellation_token.clone(),
        );
        let (response_tx, response_worker) = initialize_databricks_worker::<Response>(
            client,
            "response",
            sink_interval,
            sink_batch_size,
            cancellation_token,
        );

        let sink = DatabricksSink {
            request_tx,
            response_tx,
        };

        (sink, request_worker, response_worker)
    }
}

#[async_trait::async_trait]
impl Sink for DatabricksSink {
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

fn initialize_databricks_worker<T>(
    client: Arc<DatabricksClient>,
    event_type: &str,
    sink_interval: u64,
    sink_batch_size: usize,
    cancellation_token: CancellationToken,
) -> (UnboundedSender<T>, impl Future<Output = ()>)
where
    T: Serialize,
{
    let event_type = event_type.to_string();
    let (tx, mut rx) = unbounded_channel();
    let worker = async move {
        let mut event_buffer = Vec::with_capacity(sink_batch_size);

        while rx.recv_many(&mut event_buffer, sink_batch_size).await != 0 {
            let mut json_buffer = Vec::new();

            for it in &event_buffer {
                match serde_json::to_string(it) {
                    Ok(json) => {
                        let _ = json_buffer.write(json.as_ref());
                        let _ = json_buffer.write(NEWLINE.as_ref());
                    }
                    Err(e) => {
                        error!("Error serializing event to JSON: {:?}", e);
                    }
                }
            }

            let path = databricks_path(Utc::now(), &event_type);

            let result = client.put_file(&path, json_buffer).await;

            if let Err(e) = result {
                error!("error sending event to Databricks: {:?}", e);
            }

            event_buffer.clear();

            select! {
                _ = sleep(Duration::from_secs(sink_interval)) => {  },
                _ = cancellation_token.cancelled() => {  }
            }

            if rx.is_empty() && cancellation_token.is_cancelled() {
                break;
            }
        }
    };

    (tx, worker)
}
