//! Routing: A → B travel time with the detail the UI needs — for transit that
//! means operator / line / train type / boarding + alighting station / times /
//! stop count / transfers, plus the fastest–slowest spread; for walking, the
//! up/down elevation along the way.
//!
//! Two backends behind [`RouteProvider`]:
//!   - [`api::GoogleRoutesApi`] — `routes.googleapis.com`, needs
//!     `GOOGLE_MAPS_API_KEY`. Full structured detail. Reliable.
//!   - [`scrape::GoogleMapsScrape`] — `reqwest` only, no browser. Pulls totals
//!     (and, when it can, the transit line names) out of the Maps HTML. Fragile
//!     by nature — see the module doc.
//!
//! [`FallbackProvider`] tries the scrape first (per project direction) and
//! falls back to the API when the scrape gives nothing usable — for transit
//! that includes "a duration but no line detail".

mod api;
mod elevation;
mod estimate;
mod fallback;
mod polyline;
mod scrape;
mod types;

pub use api::GoogleRoutesApi;
pub use elevation::GsiElevation;
pub use estimate::EstimateProvider;
pub use fallback::FallbackProvider;
pub use scrape::GoogleMapsScrape;
pub use types::*;

use crate::HttpClient;
use chintai_db::entities::enums::{RouteProviderKind, TravelMode};
use chintai_db::{DatabaseConnection, RouteCacheKey, RouteCacheValue};

#[async_trait::async_trait]
pub trait RouteProvider: Send + Sync {
    /// `departure`: see [`DepartureTime`] doc — only [`GoogleRoutesApi`] can
    /// currently honour it; every other provider ignores it and computes
    /// "depart now" regardless.
    async fn route(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        departure: Option<DepartureTime>,
    ) -> Result<RouteResult, RouteError>;
}

/// The route chain. Per project direction, transit detail comes from Google
/// Maps scraping only; the API is a last-resort fallback if a key is set.
pub fn default_provider(http: HttpClient, google_api_key: Option<String>) -> FallbackProvider {
    let elevation = GsiElevation::new(http.clone());
    let mut chain: Vec<Box<dyn RouteProvider>> = vec![Box::new(GoogleMapsScrape::new(
        http.clone(),
        elevation.clone(),
    ))];
    if let Some(key) = google_api_key {
        chain.push(Box::new(GoogleRoutesApi::new(http, key, elevation)));
    }
    FallbackProvider::new(chain)
}

/// Buckets a departure time to the nearest 30 minutes, so nearby times (e.g.
/// two commute places both defaulting to 8:00 but nudged a few minutes apart)
/// still share a cache row instead of each missing forever.
fn departure_bucket(d: Option<DepartureTime>) -> Option<i32> {
    d.map(|d| {
        let mins = d.hour as i32 * 60 + d.minute as i32;
        (mins / 30) * 30
    })
}

/// Look the trip up in `route_cache`; on a miss compute it with `provider` and
/// store the result. Coordinates are rounded by the repo layer.
pub async fn route_cached(
    db: &DatabaseConnection,
    provider: &dyn RouteProvider,
    from: LatLng,
    to: LatLng,
    mode: Mode,
    departure: Option<DepartureTime>,
) -> Result<RouteResult, RouteError> {
    let key = RouteCacheKey {
        from_lat: from.lat,
        from_lng: from.lng,
        to_lat: to.lat,
        to_lng: to.lng,
        mode: mode_to_db(mode),
        departure_bucket: departure_bucket(departure),
    };

    if let Ok(Some(row)) = chintai_db::route_cache_get(db, &key).await {
        if let Some(json) = &row.result_json {
            if let Ok(cached) = serde_json::from_str::<RouteResult>(json) {
                return Ok(cached);
            }
        }
    }

    let result = provider.route(from, to, mode, departure).await?;

    // Instant estimates aren't worth a row (and would evict a real result).
    if result.provider.is_estimate() {
        return Ok(result);
    }

    let value = RouteCacheValue {
        duration_sec: result.duration.as_secs() as i32,
        transit_fastest_sec: result.transit_fastest.map(|d| d.as_secs() as i32),
        transit_slowest_sec: result.transit_slowest.map(|d| d.as_secs() as i32),
        walk_ascent_m: result.walk_ascent_m,
        walk_descent_m: result.walk_descent_m,
        provider: provider_to_db(result.provider),
        result_json: serde_json::to_string(&result).ok(),
    };
    if let Err(e) = chintai_db::route_cache_put(db, &key, value).await {
        log::warn!("route_cache_put failed (continuing): {e}");
    }
    Ok(result)
}

// `Mode` / `ProviderKind` are chintai-types and `TravelMode` /
// `RouteProviderKind` are chintai-db, so the orphan rule rules out `From`
// impls — plain functions instead.
fn mode_to_db(m: Mode) -> TravelMode {
    match m {
        Mode::Walk => TravelMode::Walk,
        Mode::Car => TravelMode::Car,
        // `route_cached` is never called with `PickupTransit` itself — the
        // engine only ever calls it for that mode's second leg, which is
        // plain `Transit`. Kept here only so the match stays exhaustive.
        Mode::Transit | Mode::PickupTransit => TravelMode::Transit,
    }
}

fn provider_to_db(p: ProviderKind) -> RouteProviderKind {
    match p {
        ProviderKind::Estimate => RouteProviderKind::Estimate,
        ProviderKind::GoogleScrape => RouteProviderKind::GoogleScrape,
        ProviderKind::GoogleApi => RouteProviderKind::GoogleApi,
    }
}
