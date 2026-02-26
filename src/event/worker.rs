use serde::Serialize;
use std::io::Write;
use std::time::Duration;
use tokio::select;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::error;

const NEWLINE: &str = "\n";

#[async_trait::async_trait]
pub(crate) trait EventUploader: Send + Sync + 'static {
    async fn upload(&self, event_type: &str, data: Vec<u8>) -> anyhow::Result<()>;
}

pub(crate) fn initialize_worker<T>(
    uploader: impl EventUploader,
    event_type: &str,
    sink_interval: u64,
    sink_batch_size: usize,
    cancellation_token: CancellationToken,
) -> (UnboundedSender<T>, impl Future<Output = ()>)
where
    T: Serialize + Send + 'static,
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

            let result = uploader.upload(&event_type, json_buffer).await;

            if let Err(e) = result {
                error!("error uploading events: {:?}", e);
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
