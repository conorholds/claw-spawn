//! HTTP server support (standalone + embeddable).
//!
//! This module mirrors the pattern used by other Cedros services:
//! - **Standalone**: `claw-spawn-server` binary calls `run()`
//! - **Embedded**: host Axum app calls `router(state)` (and may nest it)

mod state;
mod http;

pub use state::{build_state_from_env, build_state_with_pool, AppState};
pub use http::router;

use crate::infrastructure::AppConfig;
use anyhow::Context;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;

/// Standalone entrypoint for the `claw-spawn-server` binary.
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env().context("load config")?;
    let state = build_state_from_env(config.clone()).await?;

    let addr: SocketAddr = format!("{}:{}", config.server_host, config.server_port)
        .parse()
        .context("parse listen address")?;
    let listener = TcpListener::bind(addr).await.context("bind listener")?;

    info!(
        host = %config.server_host,
        port = config.server_port,
        "Server running"
    );
    info!(
        docs = %format!("http://{}:{}/docs", config.server_host, config.server_port),
        "API docs"
    );

    let app = router(state);
    axum::serve(listener, app).await.context("serve")?;
    Ok(())
}
