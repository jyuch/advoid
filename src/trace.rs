use opentelemetry;
use opentelemetry::KeyValue;
use opentelemetry::metrics::MeterProvider;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{Protocol, WithExportConfig, WithTonicConfig};
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
    service: &str,
    version: &str,
    endpoint: &str,
    api_key: Option<String>,
) -> impl MeterProvider + use<> {
    let mut builder = opentelemetry_otlp::MetricExporter::builder().with_tonic();

    if endpoint.starts_with("https://") {
        builder =
            builder.with_tls_config(tonic::transport::ClientTlsConfig::new().with_enabled_roots());
    }

    if let Some(key) = api_key {
        let mut map = opentelemetry_otlp::tonic_types::metadata::MetadataMap::new();
        map.insert("api-key", key.parse().unwrap());
        builder = builder.with_metadata(map);
    }

    let exporter = builder
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
                .with_attribute(KeyValue::new("service.name", service.to_string()))
                .with_attribute(KeyValue::new("service.version", version.to_string()))
                .build(),
        )
        .build()
}

pub fn init_tracing(
    service: &str,
    version: &str,
    endpoint: String,
    api_key: Option<String>,
) -> OtelInitGuard {
    use opentelemetry_sdk::trace::{RandomIdGenerator, Sampler};

    // Configure otel exporter.
    let mut builder = opentelemetry_otlp::SpanExporter::builder().with_tonic();

    if endpoint.starts_with("https://") {
        builder =
            builder.with_tls_config(tonic::transport::ClientTlsConfig::new().with_enabled_roots());
    }

    if let Some(ref key) = api_key {
        let mut map = opentelemetry_otlp::tonic_types::metadata::MetadataMap::new();
        map.insert("api-key", key.parse().unwrap());
        builder = builder.with_metadata(map);
    }

    let exporter = builder.with_endpoint(&endpoint).build().unwrap();

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_sampler(Sampler::AlwaysOn)
        .with_id_generator(RandomIdGenerator::default())
        .with_resource(
            Resource::builder()
                .with_attribute(KeyValue::new("service.name", service.to_string()))
                .with_attribute(KeyValue::new("service.version", version.to_string()))
                .build(),
        )
        .build();

    let tracer = tracer_provider.tracer("");

    // Compatible layer with tracing.
    let otel_trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let otel_metrics_layer = tracing_opentelemetry::MetricsLayer::new(build_meter_provider(
        service, version, &endpoint, api_key,
    ));

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
