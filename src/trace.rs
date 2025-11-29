use opentelemetry;
use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{Protocol, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::time::Duration;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub struct OtelInitGuard(SdkTracerProvider);

impl OtelInitGuard {
    pub fn new(sdk: SdkTracerProvider) -> Self {
        OtelInitGuard(sdk)
    }
}

impl Drop for OtelInitGuard {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}

fn build_meter_provider(
    service: &'static str,
    version: &'static str,
    endpoint: &str,
) -> impl MeterProvider + use<> {
    let exporter = opentelemetry_otlp::MetricExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .with_protocol(Protocol::Grpc)
        .with_timeout(Duration::from_secs(3))
        .build()
        .unwrap();

    let reader = opentelemetry_sdk::metrics::PeriodicReader::builder(exporter)
        .with_interval(Duration::from_secs(3))
        .build();

    opentelemetry_sdk::metrics::SdkMeterProvider::builder()
        .with_reader(reader)
        .with_resource(
            Resource::builder()
                .with_attribute(KeyValue::new("service.name", service))
                .with_attribute(KeyValue::new("service.version", version))
                .build(),
        )
        .build()
}

pub fn init_tracing(
    service: &'static str,
    version: &'static str,
    endpoint: String,
) -> OtelInitGuard {
    use opentelemetry_sdk::trace::{RandomIdGenerator, Sampler};

    // Configure otel exporter.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .unwrap();

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::AlwaysOn)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(
            Resource::builder()
                .with_attribute(KeyValue::new("service.name", service))
                .with_attribute(KeyValue::new("service.version", version))
                .build(),
        )
        .build();

    let tracer = tracer_provider.tracer("");

    // Compatible layer with tracing.
    let otel_trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let otel_metrics_layer =
        tracing_opentelemetry::MetricsLayer::new(build_meter_provider(service, version, &endpoint));

    tracing_subscriber::Registry::default()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(otel_trace_layer)
        .with(otel_metrics_layer)
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    OtelInitGuard::new(tracer_provider)
}

pub fn init_tracing_without_otel() {
    tracing_subscriber::Registry::default()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();
}
