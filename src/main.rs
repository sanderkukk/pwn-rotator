mod config;
mod rotator;
mod routes;

use axum::{
    routing::{get, post},
    Router,
};
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

use config::Config;
use rotator::Rotator;
use routes::{AppState, get_status, post_rotate, post_stop};

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("pwn_rotator=info".parse().unwrap()),
        )
        .init();

    let cfg = Config::from_env();

    info!(device = %cfg.device, "opening rotator connection");

    let rotator = match Rotator::open(&cfg.device, cfg.baud_rate).await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("failed to open {}: {}", cfg.device, e);
            std::process::exit(1);
        }
    };

    let state = AppState { rotator };

    let app = Router::new()
        .route("/status", get(get_status))
        .route("/rotate", post(post_rotate))
        .route("/stop", post(post_stop))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&cfg.listen_addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("failed to bind {}: {}", cfg.listen_addr, e);
            std::process::exit(1);
        });

    info!(addr = %cfg.listen_addr, "listening");
    axum::serve(listener, app).await.unwrap();
}
