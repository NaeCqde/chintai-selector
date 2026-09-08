//! Network-touching geocoder checks. Marked `#[ignore]` so `cargo test` stays
//! offline in CI; run explicitly:
//!
//!   cargo test -p chintai-scraper --test geocode_live -- --ignored --nocapture

use chintai_scraper::geocode::{Geocoder, NominatimGeocoder};
use chintai_scraper::HttpClient;

#[tokio::test]
#[ignore = "hits nominatim.openstreetmap.org"]
async fn nominatim_finds_tokyo_tower() {
    let g = NominatimGeocoder::new(HttpClient::default());
    let hit = g
        .geocode("東京タワー")
        .await
        .expect("request ok")
        .expect("a result");

    eprintln!("{hit:?}");
    // Tokyo Tower is at ~35.6586, 139.7454.
    assert!((hit.lat - 35.6586).abs() < 0.02, "lat {}", hit.lat);
    assert!((hit.lng - 139.7454).abs() < 0.02, "lng {}", hit.lng);
    assert_eq!(hit.source, "nominatim");
}

#[tokio::test]
#[ignore = "hits nominatim.openstreetmap.org"]
async fn nominatim_address_lookup() {
    let g = NominatimGeocoder::new(HttpClient::default());
    let hit = g
        .geocode("東京都千代田区丸の内1-9-1")
        .await
        .expect("request ok")
        .expect("a result");
    eprintln!("{hit:?}");
    // Tokyo Station area.
    assert!((hit.lat - 35.681).abs() < 0.05);
    assert!((hit.lng - 139.767).abs() < 0.05);
}
