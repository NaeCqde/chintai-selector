use super::*;
use chintai_db::entities::enums::{BrandKind, ListingSite, PriceTier};
use chintai_db::*;
use chintai_scraper::route::{Itinerary, LatLng, Mode, ProviderKind, RouteError, RouteResult};
use std::time::Duration;

/// Straight-line distance at a fixed speed per mode — deterministic, no network.
struct MockProvider;

#[async_trait::async_trait]
impl RouteProvider for MockProvider {
    async fn route(&self, from: LatLng, to: LatLng, mode: Mode) -> Result<RouteResult, RouteError> {
        let m = super::geo::haversine_m(from.lat, from.lng, to.lat, to.lng);
        let mps = match mode {
            Mode::Walk => 1.3,
            Mode::Car => 8.0,
            Mode::Transit => 6.0,
        };
        let dur = Duration::from_secs((m / mps) as u64);
        Ok(RouteResult::single(
            mode,
            ProviderKind::GoogleApi,
            Itinerary {
                duration: dur,
                depart: None,
                arrive: None,
                fare_yen: None,
                transfers: 0,
                legs: vec![],
            },
        ))
    }
}

async fn seed() -> DatabaseConnection {
    let db = connect("sqlite::memory:").await.unwrap();

    let conveni = upsert_brand(
        &db,
        "ローソン",
        BrandKind::ConvenienceStore,
        PriceTier::Expensive,
    )
    .await
    .unwrap();
    let cheap = upsert_brand(
        &db,
        "業務スーパー",
        BrandKind::Supermarket,
        PriceTier::Cheap,
    )
    .await
    .unwrap();

    // Near property A (≈120 m away).
    upsert_store(
        &db,
        NewStore {
            brand_id: conveni.id,
            name: "ローソン A前".into(),
            lat: Some(35.6585),
            lng: Some(139.7000),
            source: Some(chintai_db::entities::enums::StoreSource::Osm),
            external_id: Some("node/1".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    upsert_store(
        &db,
        NewStore {
            brand_id: cheap.id,
            name: "業務スーパー A".into(),
            lat: Some(35.6600),
            lng: Some(139.7010),
            source: Some(chintai_db::entities::enums::StoreSource::Osm),
            external_id: Some("node/2".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Property A: right by Shibuya (35.6580,139.7016).
    let a = upsert_property(
        &db,
        NewProperty {
            name: "近マンション".into(),
            address: "渋谷区A".into(),
            lat: Some(35.6584),
            lng: Some(139.7008),
            built_year: Some(2015),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    for (key, rent, area) in [
        ("2f|1k|25|80", 80_000, 25.0),
        ("5f|1ldk|40|140", 140_000, 40.0),
    ] {
        let u = upsert_unit(
            &db,
            NewUnit {
                property_id: a.id,
                dedup_key: key.into(),
                layout: Some(if rent < 100_000 { "1K" } else { "1LDK" }.into()),
                area_m2: Some(area),
                rent_yen: Some(rent),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        record_listing(
            &db,
            NewListing {
                unit_id: Some(u.id),
                site: ListingSite::Suumo,
                site_listing_id: format!("s-{key}"),
                url: format!("https://suumo.jp/chintai/bc_{key}/"),
                raw_json: "{}".into(),
            },
        )
        .await
        .unwrap();
    }

    // Property B: ~4 km away (near Shinjuku), old, no nearby POIs.
    let b = upsert_property(
        &db,
        NewProperty {
            name: "遠アパート".into(),
            address: "新宿区B".into(),
            lat: Some(35.6900),
            lng: Some(139.7000),
            built_year: Some(1985),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    upsert_unit(
        &db,
        NewUnit {
            property_id: b.id,
            dedup_key: "1f|1k|20|70".into(),
            layout: Some("1K".into()),
            area_m2: Some(20.0),
            rent_yen: Some(70_000),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    db
}

fn shibuya_place(max: u32, mode: Mode) -> PlaceSpec {
    PlaceSpec {
        label: "職場".into(),
        lat: 35.6580,
        lng: 139.7016,
        mode,
        max_minutes: max,
    }
}

#[tokio::test]
async fn time_limit_drops_the_far_property() {
    let db = seed().await;
    let engine = Engine::new(&db, &MockProvider);

    let req = SearchRequest {
        places: vec![shibuya_place(20, Mode::Walk)],
        filters: Filters::default(),
        ranking: vec![],
        limit: 10,
    };
    let resp = engine.search(&req).await.unwrap();

    assert_eq!(resp.considered, 2);
    assert_eq!(resp.matched, 1);
    assert_eq!(resp.results[0].name, "近マンション");
    // best unit is the cheaper one
    assert_eq!(resp.results[0].best_unit.rent_yen, Some(80_000));
    // it found the nearby conveni + cheap super
    assert!(resp.results[0].nearest_conveni_m.unwrap() < 300.0);
    assert!(resp.results[0].nearest_cheap_super_m.is_some());
    // and both listing sources on the best unit
    assert_eq!(resp.results[0].best_unit.sources.len(), 1);
}

#[tokio::test]
async fn rent_filter_and_layout_filter() {
    let db = seed().await;
    let engine = Engine::new(&db, &MockProvider);

    let req = SearchRequest {
        places: vec![shibuya_place(60, Mode::Transit)],
        filters: Filters {
            max_rent_yen: Some(90_000),
            layouts: vec!["1K".into()],
            ..Default::default()
        },
        ranking: vec![],
        limit: 10,
    };
    let resp = engine.search(&req).await.unwrap();
    // Both properties have a sub-90k 1K unit and are within 60 min transit.
    assert_eq!(resp.matched, 2);
    for r in &resp.results {
        assert!(r.best_unit.rent_yen.unwrap() <= 90_000);
        assert_eq!(r.best_unit.layout.as_deref(), Some("1K"));
    }
}

#[tokio::test]
async fn conveni_walk_filter_drops_property_b() {
    let db = seed().await;
    let engine = Engine::new(&db, &MockProvider);

    let req = SearchRequest {
        places: vec![shibuya_place(90, Mode::Transit)],
        filters: Filters {
            max_conveni_walk_min: Some(5),
            ..Default::default()
        },
        ranking: vec![],
        limit: 10,
    };
    let resp = engine.search(&req).await.unwrap();
    assert_eq!(resp.matched, 1);
    assert_eq!(resp.results[0].name, "近マンション");
}

#[tokio::test]
async fn ranking_by_area_puts_bigger_first() {
    let db = seed().await;
    let engine = Engine::new(&db, &MockProvider);

    let req = SearchRequest {
        places: vec![shibuya_place(120, Mode::Transit)],
        filters: Filters::default(),
        ranking: vec![RankSpec::new(RankKey::Area, 1.0)],
        limit: 10,
    };
    let resp = engine.search(&req).await.unwrap();
    assert_eq!(resp.results[0].name, "近マンション"); // has the 40 m² unit
    assert_eq!(resp.results[0].rank, 1);
    assert!(resp.results[0].score >= resp.results[1].score);
}

#[tokio::test]
async fn route_detail_is_carried_through() {
    let db = seed().await;
    let engine = Engine::new(&db, &MockProvider);
    let req = SearchRequest {
        places: vec![shibuya_place(30, Mode::Walk)],
        filters: Filters::default(),
        ranking: vec![],
        limit: 5,
    };
    let resp = engine.search(&req).await.unwrap();
    let pa = &resp.results[0].places[0];
    assert_eq!(pa.label, "職場");
    assert!(pa.within_limit);
    assert_eq!(pa.route.mode, Mode::Walk);
}
