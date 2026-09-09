//! Instant, network-free travel-time estimate from crow-flies distance. Used by
//! the ranking engine to filter/rank hundreds of candidates in one pass; the
//! real provider then refines only the handful that end up on screen.

use std::time::Duration;

use super::types::*;
use super::RouteProvider;

pub struct EstimateProvider;

fn haversine_m(a: LatLng, b: LatLng) -> f64 {
    const R: f64 = 6_371_000.0;
    let (p1, p2) = (a.lat.to_radians(), b.lat.to_radians());
    let dp = (b.lat - a.lat).to_radians();
    let dl = (b.lng - a.lng).to_radians();
    let x = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * x.sqrt().asin()
}

#[async_trait::async_trait]
impl RouteProvider for EstimateProvider {
    async fn route(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        _departure: Option<DepartureTime>,
    ) -> Result<RouteResult, RouteError> {
        let straight = haversine_m(from, to);
        // detour factor, effective m/s, fixed overhead (wait/access)
        let (detour, mps, overhead_s) = match mode {
            Mode::Walk => (1.30, 1.30, 0.0),
            Mode::Car => (1.45, 8.5, 120.0),
            // PickupTransit never actually reaches here in practice — the
            // engine estimates it itself, from the property's own
            // precomputed station-car time, before this provider is called.
            Mode::Transit | Mode::PickupTransit => (1.25, 9.0, 360.0),
        };
        let secs = (straight * detour / mps + overhead_s).round() as u64;
        let dur = Duration::from_secs(secs.max(60));

        let mut out = RouteResult::single(
            mode,
            ProviderKind::Estimate,
            Itinerary {
                duration: dur,
                depart: None,
                arrive: None,
                fare_yen: None,
                transfers: 0,
                legs: Vec::new(),
            },
        );
        out.distance_m = Some(straight * detour);
        if mode == Mode::Transit {
            // Rough spread so the UI still shows a range.
            out.transit_fastest = Some(dur.mul_f64(0.85));
            out.transit_slowest = Some(dur.mul_f64(1.3));
        }
        Ok(out)
    }
}
