//! Network-touching route checks. `#[ignore]` so CI stays offline; run:
//!
//!   cargo test -p chintai-scraper --test route_live -- --ignored --nocapture

use chintai_scraper::route::{DepartureTime, GoogleMapsScrape, GsiElevation, LatLng, Mode, RouteProvider};
use chintai_scraper::HttpClient;

// 渋谷駅 -> 恵比寿駅 あたり（歩くと坂がある）。
const SHIBUYA: LatLng = LatLng {
    lat: 35.658,
    lng: 139.7016,
};
const EBISU: LatLng = LatLng {
    lat: 35.6467,
    lng: 139.71,
};

#[tokio::test]
#[ignore = "hits cyberjapandata2.gsi.go.jp"]
async fn gsi_elevation_reports_a_delta() {
    let gsi = GsiElevation::new(HttpClient::default());
    let path = vec![
        SHIBUYA,
        LatLng {
            lat: 35.652,
            lng: 139.706,
        },
        EBISU,
    ];
    let delta = gsi.delta_for_path(&path).await.expect("some samples");
    eprintln!("{delta:?}");
    assert!(delta.ascent_m + delta.descent_m > 0.0);
}

#[tokio::test]
#[ignore = "hits www.google.com"]
async fn scrape_walk_duration_is_plausible() {
    let http = HttpClient::default();
    let p = GoogleMapsScrape::new(http.clone(), GsiElevation::new(http));
    match p.route(SHIBUYA, EBISU, Mode::Walk, None).await {
        Ok(r) => {
            eprintln!(
                "walk {}s  asc={:?} desc={:?}",
                r.duration.as_secs(),
                r.walk_ascent_m,
                r.walk_descent_m
            );
            // ~1.6 km on foot -> roughly 10–40 min.
            assert!(r.duration.as_secs() >= 300 && r.duration.as_secs() <= 3600);
        }
        Err(e) => panic!("scrape walk failed: {e}"),
    }
}

#[tokio::test]
#[ignore = "hits www.google.com"]
async fn scrape_transit_returns_totals() {
    let http = HttpClient::default();
    let p = GoogleMapsScrape::new(http.clone(), GsiElevation::new(http));
    match p.route(SHIBUYA, EBISU, Mode::Transit, None).await {
        Ok(r) => eprintln!(
            "transit fastest={:?} slowest={:?}",
            r.transit_fastest, r.transit_slowest
        ),
        Err(e) => eprintln!("scrape transit gave nothing (expected-ish): {e}"),
    }
}

/// The scrape path can honour a departure time (see `scrape::patch_departure_time`'s
/// doc) — a real request should come back with itinerary departure clocks
/// near the requested time, not "now".
#[tokio::test]
#[ignore = "hits www.google.com"]
async fn scrape_transit_honours_departure_time() {
    let http = HttpClient::default();
    let p = GoogleMapsScrape::new(http.clone(), GsiElevation::new(http));
    let requested = DepartureTime::new(8, 0);
    match p
        .route(SHIBUYA, EBISU, Mode::Transit, Some(requested))
        .await
    {
        Ok(r) => {
            // Not necessarily the *first* itinerary: a short trip like this
            // can have a walk-only "alternative" with no transit leg (and so
            // no departure clock) sort first by duration. Any itinerary
            // showing a departure clock is enough to confirm patching works.
            let depart = r
                .itineraries
                .iter()
                .find_map(|it| it.depart.clone())
                .expect("at least one transit itinerary with a departure clock time");
            eprintln!("requested 08:00, got depart={depart}");
            let hour: u32 = depart
                .split(':')
                .next()
                .and_then(|h| h.parse().ok())
                .expect("HH:MM");
            // Loose window (not exactly ==8) since Google may offer the
            // nearest available departure a few minutes either side.
            assert!(
                (7..=9).contains(&hour),
                "departure {depart} is nowhere near the requested 08:00"
            );
        }
        Err(e) => panic!("scrape transit with departure time failed: {e}"),
    }
}
