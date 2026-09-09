use super::*;
use chintai_db::entities::enums::{BrandKind, ListingSite, PriceTier};
use chintai_db::*;
use chintai_scraper::route::{
    DepartureTime, Itinerary, LatLng, Mode, ProviderKind, RouteError, RouteResult,
};
use std::time::Duration;

/// Straight-line distance at a fixed speed per mode — deterministic, no network.
struct MockProvider;

#[async_trait::async_trait]
impl RouteProvider for MockProvider {
    async fn route(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        _departure: Option<DepartureTime>,
    ) -> Result<RouteResult, RouteError> {
        let m = super::geo::haversine_m(from.lat, from.lng, to.lat, to.lng);
        let mps = match mode {
            Mode::Walk => 1.3,
            Mode::Car => 8.0,
            Mode::Transit | Mode::PickupTransit => 6.0,
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
        kind: chintai_types::PlaceKind::Commute,
        departure: DepartureTime::new(8, 0),
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
        refine: false,
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
        refine: false,
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
        refine: false,
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
        refine: false,
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
        refine: false,
    };
    let resp = engine.search(&req).await.unwrap();
    let pa = &resp.results[0].places[0];
    assert_eq!(pa.label, "職場");
    assert!(pa.within_limit);
    assert_eq!(pa.route.mode, Mode::Walk);
}

fn shibuya_pickup_place(max: u32) -> PlaceSpec {
    PlaceSpec {
        label: "職場".into(),
        lat: 35.6580,
        lng: 139.7016,
        mode: Mode::PickupTransit,
        max_minutes: max,
        kind: chintai_types::PlaceKind::Commute,
        departure: DepartureTime::new(8, 0),
    }
}

/// 送迎モード: the car-to-station leg is fixed precomputed data (never a
/// live call — `MockProvider` would give a different, much slower number if
/// it were used for that leg), the transit-from-station leg is live/mocked,
/// and the two combine into one result with a drive leg spliced onto the
/// front of the itinerary.
#[tokio::test]
async fn pickup_transit_combines_precomputed_car_leg_with_live_transit() {
    use chintai_db::entities::property;
    use chintai_db::sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    let db = seed().await;
    let a = property::Entity::find()
        .filter(property::Column::Name.eq("近マンション"))
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    // Precomputed: a 3-minute drive to a station 1km away (well outside
    // MockProvider's car speed, so if this leg were computed live instead of
    // read from the DB, the duration assertion below would fail).
    chintai_db::set_property_station_time(
        &db, a.id, /*walk_min*/ 12, /*distance_m*/ Some(900.0),
        /*car_min*/ Some(3), /*station_lat*/ 35.6700, /*station_lng*/ 139.7100,
    )
    .await
    .unwrap();

    let engine = Engine::new(&db, &MockProvider);
    let req = SearchRequest {
        places: vec![shibuya_pickup_place(60)],
        filters: Filters::default(),
        ranking: vec![],
        limit: 5,
        refine: false,
    };
    let resp = engine.search(&req).await.unwrap();
    let found = resp
        .results
        .iter()
        .find(|r| r.name == "近マンション")
        .expect("property with a precomputed car leg must be reachable");
    let pa = &found.places[0];
    assert_eq!(pa.route.mode, Mode::PickupTransit);

    // MockProvider gives station(35.67,139.71)->place(35.658,139.7016) at
    // 6 m/s; ~2.9 km apart -> ~8 min. Plus the fixed 3-minute car leg.
    let live_leg_minutes = (pa.minutes as i64 - 3).max(0);
    assert!(
        (5..=12).contains(&live_leg_minutes),
        "unexpected live-leg minutes: {live_leg_minutes} (total {})",
        pa.minutes
    );

    let first_leg = pa
        .route
        .itineraries
        .first()
        .and_then(|it| it.legs.first())
        .expect("combined itinerary must have at least the drive leg");
    assert!(
        matches!(first_leg, Leg::Drive(d) if d.duration.as_secs() == 180),
        "expected a 3-minute drive leg first, got {first_leg:?}"
    );
}

/// A property whose car-to-station leg hasn't been precomputed yet must not
/// be silently treated as reachable under 送迎モード.
#[tokio::test]
async fn pickup_transit_excludes_properties_without_a_precomputed_car_leg() {
    let db = seed().await;
    // Property A never gets `set_property_station_time` called on it here,
    // so `station_car_min`/`station_lat`/`station_lng` stay NULL.
    let engine = Engine::new(&db, &MockProvider);
    let req = SearchRequest {
        places: vec![shibuya_pickup_place(120)],
        filters: Filters::default(),
        ranking: vec![],
        limit: 5,
        refine: false,
    };
    let resp = engine.search(&req).await.unwrap();
    assert!(
        resp.results.iter().all(|r| r.name != "近マンション"),
        "property without a precomputed car leg should be dropped, not guessed at"
    );
}
