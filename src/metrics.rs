use axum::routing::get;
use axum::Router;
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use std::future::ready;
use std::net::SocketAddr;
use tokio::net::TcpListener;

pub(crate) async fn start_metrics_server(endpoint: SocketAddr) -> anyhow::Result<()> {
    let app = metrics_app()?;
    let listener = TcpListener::bind(endpoint).await?;

    tracing::debug!("listening on {}", listener.local_addr()?);
    axum::serve(listener, app).await?;
    Ok(())
}

fn metrics_app() -> anyhow::Result<Router> {
    let recorder_handle = setup_metrics_recorder()?;
    let router = Router::new().route("/metrics", get(move || ready(recorder_handle.render())));
    Ok(router)
}

fn setup_metrics_recorder() -> anyhow::Result<PrometheusHandle> {
    let handle = PrometheusBuilder::new().install_recorder()?;

    Ok(handle)
}
