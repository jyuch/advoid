use opentelemetry::metrics::MeterProvider;
use opentelemetry_otlp::WithExportConfig;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pub(crate) struct OtelInitGuard();

impl Drop for OtelInitGuard {
    fn drop(&mut self) {
        opentelemetry::global::shutdown_tracer_provider();
    }
}

fn build_meter_provider(endpoint: String) -> impl MeterProvider {
    opentelemetry_otlp::new_pipeline()
        .metrics(opentelemetry_sdk::runtime::Tokio)
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(endpoint),
        )
        .build()
        .expect("Failed to build metrics controller")
}

pub(crate) fn init_tracing(
    service: &'static str,
    version: &'static str,
    endpoint: String,
) -> OtelInitGuard {
    use opentelemetry_sdk::trace::{RandomIdGenerator, Sampler};

    // Configure otel exporter.
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(
            opentelemetry_otlp::new_exporter()
                .tonic()
                .with_endpoint(endpoint.clone()),
        )
        .with_trace_config(
            opentelemetry_sdk::trace::config()
                .with_sampler(Sampler::AlwaysOn)
                .with_id_generator(RandomIdGenerator::default())
                .with_resource(opentelemetry_sdk::Resource::new(vec![
                    opentelemetry::KeyValue::new("service.name", service),
                    opentelemetry::KeyValue::new("service.version", version),
                ])),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)
        // .install_simple()
        .expect("Not running in tokio runtime");

    // Compatible layer with tracing.
    let otel_trace_layer = tracing_opentelemetry::layer().with_tracer(tracer);
    let otel_metrics_layer =
        tracing_opentelemetry::MetricsLayer::new(build_meter_provider(endpoint.clone()));

    tracing_subscriber::Registry::default()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(otel_trace_layer)
        .with(otel_metrics_layer)
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();

    OtelInitGuard()
}

pub(crate) fn init_tracing_without_otel() {
    tracing_subscriber::Registry::default()
        .with(tracing_subscriber::fmt::Layer::new())
        .with(tracing_subscriber::filter::EnvFilter::from_default_env())
        .init();
}
