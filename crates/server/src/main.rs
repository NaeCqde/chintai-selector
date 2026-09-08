//! chintai-server — Leptos SSR app + JSON API + the filter/ranking engine.
//!
//! The Leptos UI and the search engine land in later milestones. For now this
//! is a bare axum server exposing a health check, wired to the same crates the
//! full app will use so the workspace dependency graph is exercised.

use axum::{routing::get, Router};
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let app = Router::new().route("/api/health", get(|| async { "ok" }));

    let addr: SocketAddr = std::env::var("LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse()?;
    log::info!("chintai-server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
