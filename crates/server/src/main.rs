//! chintai-server — Leptos SSR app + JSON API + the filter/ranking engine.
//!
//! The Leptos UI lands in the next milestone. For now this is an axum server
//! exposing the engine over JSON so it can be exercised end-to-end.

mod engine;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use chintai_db::DatabaseConnection;
use chintai_scraper::route::{FallbackProvider, RouteProvider};

use engine::{Engine, SearchRequest};

struct AppState {
    db: DatabaseConnection,
    routes: FallbackProvider,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let db_url = std::env::var("CHINTAI_DB")
        .unwrap_or_else(|_| "sqlite://chintai.sqlite?mode=rwc".to_string());
    let db = chintai_db::connect(&db_url).await?;
    let routes = chintai_scraper::route::default_provider(
        chintai_scraper::HttpClient::default(),
        chintai_scraper::google_api_key(),
    );
    let state = Arc::new(AppState { db, routes });

    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/search", post(search))
        .with_state(state);

    let addr: SocketAddr = std::env::var("LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse()?;
    log::info!("chintai-server listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    let provider: &dyn RouteProvider = &state.routes;
    match Engine::new(&state.db, provider).search(&req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            log::error!("search failed: {e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response()
        }
    }
}
