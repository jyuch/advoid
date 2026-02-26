use super::Sink;
use uuid::Uuid;

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
