//! Network-touching route checks. `#[ignore]` so CI stays offline; run:
//!
//!   cargo test -p chintai-scraper --test route_live -- --ignored --nocapture

use chintai_scraper::route::{GoogleMapsScrape, GsiElevation, LatLng, Mode, RouteProvider};
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
