//! JSON API handlers. Thin wrappers over the engine, the DB, and the geocoder.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use chintai_db::entities::enums::{BrandKind, PriceTier};
use chintai_scraper::geocode::Geocoder;
use chintai_types::{Geocoded, PoiKind, PoiPoint, SearchRequest};
use serde::Deserialize;

use crate::engine::Engine;
use crate::AppState;

/// Input-size caps: each place triggers external route lookups.
const MAX_PLACES: usize = 10;
const MAX_LIMIT: usize = 500;

pub async fn search(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SearchRequest>,
) -> impl IntoResponse {
    if req.places.len() > MAX_PLACES || req.limit as usize > MAX_LIMIT {
        return (
            StatusCode::BAD_REQUEST,
            "too many places or limit too large",
        )
            .into_response();
    }
    match Engine::new(&state.db, &state.routes).search(&req).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => {
            log::error!("search: {e:#}");
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct BboxQuery {
    /// "south,west,north,east"
    bbox: String,
    #[serde(default)]
    kinds: Option<String>,
}

pub async fn pois(
    State(state): State<Arc<AppState>>,
    Query(q): Query<BboxQuery>,
) -> impl IntoResponse {
    let Some(bbox) = parse_bbox(&q.bbox) else {
        return (StatusCode::BAD_REQUEST, "bbox must be s,w,n,e").into_response();
    };
    // Guard against someone asking for the whole country at once.
    let area = (bbox.2 - bbox.0) * (bbox.3 - bbox.1);
    if !area.is_finite() || area < 0.0 || area > 1.0 {
        return (StatusCode::BAD_REQUEST, "bbox too large").into_response();
    }
    let want: Vec<&str> = q
        .kinds
        .as_deref()
        .map(|s| s.split(',').collect())
        .unwrap_or_else(|| vec!["convenience", "supermarket", "delivery", "station"]);
    let has = |k: &str| want.contains(&k);

    let mut out: Vec<PoiPoint> = Vec::new();

    if has("convenience") || has("supermarket") {
        match chintai_db::stores_in_bbox(&state.db, bbox).await {
            Ok(rows) => {
                for (s, b) in rows {
                    let kind = match b.kind {
                        BrandKind::ConvenienceStore => PoiKind::Convenience,
                        BrandKind::Supermarket if b.price_tier == PriceTier::Cheap => {
                            PoiKind::CheapSupermarket
                        }
                        BrandKind::Supermarket => PoiKind::Supermarket,
                    };
                    let show = match kind {
                        PoiKind::Convenience => has("convenience"),
                        _ => has("supermarket"),
                    };
                    if show {
                        out.push(PoiPoint {
                            kind,
                            name: s.name,
                            lat: s.lat.unwrap_or_default(),
                            lng: s.lng.unwrap_or_default(),
                            brand: Some(b.name),
                        });
                    }
                }
            }
            Err(e) => log::warn!("stores_in_bbox: {e}"),
        }
    }
    if has("delivery") {
        if let Ok(rows) = chintai_db::deliveries_in_bbox(&state.db, bbox).await {
            for d in rows {
                out.push(PoiPoint {
                    kind: PoiKind::Delivery,
                    name: d.name.unwrap_or_else(|| d.carrier.clone()),
                    lat: d.lat.unwrap_or_default(),
                    lng: d.lng.unwrap_or_default(),
                    brand: Some(d.carrier),
                });
            }
        }
    }
    if has("station") {
        if let Ok(rows) = chintai_db::stations_in_bbox(&state.db, bbox).await {
            for st in rows {
                out.push(PoiPoint {
                    kind: PoiKind::Station,
                    name: st.name,
                    lat: st.lat,
                    lng: st.lng,
                    brand: None,
                });
            }
        }
    }

    Json(out).into_response()
}

#[derive(Deserialize)]
pub struct GeocodeQuery {
    q: String,
}

pub async fn geocode(
    State(state): State<Arc<AppState>>,
    Query(q): Query<GeocodeQuery>,
) -> impl IntoResponse {
    match state.geocoder.geocode(&q.q).await {
        Ok(Some(hit)) => Json(Some::<Geocoded>(hit)).into_response(),
        Ok(None) => Json(None::<Geocoded>).into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, format!("{e}")).into_response(),
    }
}

fn parse_bbox(s: &str) -> Option<(f64, f64, f64, f64)> {
    let n: Vec<f64> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
    (n.len() == 4).then_some((n[0], n[1], n[2], n[3]))
}
