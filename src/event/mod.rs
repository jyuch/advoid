mod channel;
mod worker;

mod stub;
pub use stub::StubSink;

mod s3;
pub use s3::S3Sink;

mod databricks;
pub use databricks::DatabricksSink;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
