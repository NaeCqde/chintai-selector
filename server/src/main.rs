//! chintai-server — JSON API + the filter/ranking engine, and static host for
//! the `chintai-web` WASM SPA (built separately into `web/dist`).

mod api;
mod engine;

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    routing::{get, post},
    Router,
};
use chintai_db::DatabaseConnection;
use chintai_scraper::geocode::{default_geocoder, FallbackGeocoder};
use chintai_scraper::route::FallbackProvider;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub db: DatabaseConnection,
    pub routes: FallbackProvider,
    pub geocoder: FallbackGeocoder,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init_from_env(env_logger::Env::new().default_filter_or("info"));

    let db_url = std::env::var("CHINTAI_DB")
        .unwrap_or_else(|_| "sqlite://chintai.sqlite?mode=rwc".to_string());
    let db = chintai_db::connect(&db_url).await?;

    let http = chintai_scraper::HttpClient::default();
    let api_key = chintai_scraper::google_api_key();
    let routes = chintai_scraper::route::default_provider(http.clone(), api_key.clone());
    let geocoder = default_geocoder(http, api_key, false);

    let state = Arc::new(AppState {
        db,
        routes,
        geocoder,
    });

    let web_dir = std::env::var("CHINTAI_WEB_DIR").unwrap_or_else(|_| "web/dist".to_string());
    let index = format!("{web_dir}/index.html");
    let static_files = ServeDir::new(&web_dir).fallback(ServeFile::new(index));

    let app = Router::new()
        .route("/api/health", get(|| async { "ok" }))
        .route("/api/search", post(api::search))
        .route("/api/pois", get(api::pois))
        .route("/api/geocode", get(api::geocode))
        .with_state(state)
        .fallback_service(static_files)
        .layer(CorsLayer::permissive());

    let addr: SocketAddr = std::env::var("LISTEN")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_string())
        .parse()?;
    log::info!("chintai-server on http://{addr}  (web dir: {web_dir})");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
