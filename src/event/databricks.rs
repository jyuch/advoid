use super::Sink;
use super::channel::ChannelSink;
use super::worker::{EventUploader, initialize_worker};
use super::{Request, Response};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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

struct DatabricksUploader {
    client: Arc<DatabricksClient>,
}

#[async_trait::async_trait]
impl EventUploader for DatabricksUploader {
    async fn upload(&self, event_type: &str, data: Vec<u8>) -> anyhow::Result<()> {
        let path = databricks_path(Utc::now(), event_type);
        self.client.put_file(&path, data).await
    }
}

fn databricks_path(occur: DateTime<Utc>, event_type: &str) -> String {
    let id = Uuid::now_v7();
    format!("{}/{}/{}.json", event_type, occur.format("%Y-%m-%d"), id)
}

pub struct DatabricksSink {
    inner: ChannelSink,
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

        let request_uploader = DatabricksUploader {
            client: client.clone(),
        };
        let response_uploader = DatabricksUploader { client };

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

        let sink = DatabricksSink {
            inner: ChannelSink::new(request_tx, response_tx),
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
        self.inner
            .request(src_ip, src_port, name, query_class, query_type, op_code)
            .await
    }

    async fn response(&self, id: Uuid, response_code: String) {
        self.inner.response(id, response_code).await
    }
}
