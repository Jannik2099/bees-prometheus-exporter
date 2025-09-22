use anyhow::Context;
use axum::{Router, response::Html, routing::get};
use log::{error, info};
use prometheus_client::registry::Registry;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

async fn handler(registry: Arc<Mutex<Registry>>) -> String {
    let mut buffer = String::new();
    {
        let registry = registry.lock().unwrap();
        match prometheus_client::encoding::text::encode(&mut buffer, &registry) {
            Ok(_) => {}
            Err(e) => {
                error!("Failed to encode metrics: {}", e);
                buffer = String::new();
            }
        }
    }
    buffer
}

async fn root_handler() -> Html<&'static str> {
    Html(
        r#"
<!DOCTYPE html>
<html>
<head>
    <title>Bees Prometheus Exporter</title>
</head>
<body>
    <h1>Bees Prometheus Exporter</h1>
    <p><a href="/metrics">Metrics</a></p>
</body>
</html>
    "#,
    )
}

fn init_app(registry: Arc<Mutex<Registry>>) -> Router {
    Router::new().route("/", get(root_handler)).route(
        "/metrics",
        get({
            let registry = registry.clone();
            move || async move { handler(registry.clone()).await }
        }),
    )
}

pub async fn start_server(
    registry: Arc<Mutex<Registry>>,
    address: &str,
    port: u16,
) -> anyhow::Result<()> {
    let app = init_app(registry);
    let listener = TcpListener::bind((address, port))
        .await
        .with_context(|| format!("Failed to bind to {}:{}", address, port))?;

    info!("Listening on http://{}:{}", address, port);
    axum::serve(listener, app).await.context("Server error")?;

    Ok(())
}
