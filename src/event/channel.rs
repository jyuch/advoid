use super::{Request, Response, Sink};
use chrono::Utc;
use tokio::sync::mpsc::UnboundedSender;
use tracing::error;
use uuid::Uuid;

pub(crate) struct ChannelSink {
    request_tx: UnboundedSender<Request>,
    response_tx: UnboundedSender<Response>,
}

impl ChannelSink {
    pub(crate) fn new(
        request_tx: UnboundedSender<Request>,
        response_tx: UnboundedSender<Response>,
    ) -> Self {
        Self {
            request_tx,
            response_tx,
        }
    }
}

#[async_trait::async_trait]
impl Sink for ChannelSink {
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
