//! End-to-end exercise of the schema + repository helpers against a real
//! (in-memory) SQLite database: migrations run, upserts converge on their
//! unique index, and the route cache keys on rounded coordinates.

use chintai_db::entities::enums::*;
use chintai_db::entities::{delivery, route_cache, store};
use chintai_db::sea_orm::EntityTrait;
use chintai_db::*;

async fn db() -> DatabaseConnection {
    connect("sqlite::memory:").await.expect("connect + migrate")
}

#[tokio::test]
async fn brand_upsert_is_idempotent_and_updates() {
    let db = db().await;

    let a = upsert_brand(
        &db,
        "ローソン",
        BrandKind::ConvenienceStore,
        PriceTier::Expensive,
    )
    .await
    .unwrap();
    // Same name again, different tier -> same row, tier updated.
    let b = upsert_brand(
        &db,
        "ローソン",
        BrandKind::ConvenienceStore,
        PriceTier::Cheap,
    )
    .await
    .unwrap();

    assert_eq!(a.id, b.id);
    assert_eq!(b.price_tier, PriceTier::Cheap);
    assert_eq!(
        count(&db, chintai_db::entities::brand::Entity::find())
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn store_upserts_on_source_external_id() {
    let db = db().await;
    let brand = upsert_brand(
        &db,
        "セブン-イレブン",
        BrandKind::ConvenienceStore,
        PriceTier::Expensive,
    )
    .await
    .unwrap();

    let first = upsert_store(
        &db,
        NewStore {
            brand_id: brand.id,
            name: "セブンイレブン渋谷駅前店".into(),
            lat: Some(35.6595),
            lng: Some(139.7005),
            address: Some("東京都渋谷区".into()),
            source: Some(StoreSource::BrandSite),
            external_id: Some("711-000123".into()),
        },
    )
    .await
    .unwrap();

    // Re-crawl: same external id, moved coordinates -> same row, coords updated.
    let again = upsert_store(
        &db,
        NewStore {
            brand_id: brand.id,
            name: "セブンイレブン渋谷駅前店".into(),
            lat: Some(35.6596),
            lng: Some(139.7006),
            address: Some("東京都渋谷区道玄坂".into()),
            source: Some(StoreSource::BrandSite),
            external_id: Some("711-000123".into()),
        },
    )
    .await
    .unwrap();

    assert_eq!(first.id, again.id);
    assert_eq!(again.lat, Some(35.6596));
    assert_eq!(count(&db, store::Entity::find()).await.unwrap(), 1);

    // Two coordinate-only rows (no external id) are intentionally kept apart;
    // they get de-duplicated by proximity later.
    for _ in 0..2 {
        upsert_store(
            &db,
            NewStore {
                brand_id: brand.id,
                name: "セブンイレブン名無し店".into(),
                lat: Some(35.7),
                lng: Some(139.7),
                source: Some(StoreSource::Osm),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }
    assert_eq!(count(&db, store::Entity::find()).await.unwrap(), 3);
}

#[tokio::test]
async fn delivery_links_to_store_and_standalone() {
    let db = db().await;
    let brand = upsert_brand(
        &db,
        "ヤマト運輸",
        BrandKind::ConvenienceStore,
        PriceTier::Expensive,
    )
    .await
    .unwrap();
    let s = upsert_store(
        &db,
        NewStore {
            brand_id: brand.id,
            name: "宅急便センター".into(),
            source: Some(StoreSource::BrandSite),
            external_id: Some("yamato-042".into()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let counter = upsert_delivery(
        &db,
        NewDelivery {
            store_id: Some(s.id),
            carrier: "yamato".into(),
            external_id: Some("yamato-042".into()),
            can_ship: true,
            can_receive: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(counter.store_id, Some(s.id));

    let locker = upsert_delivery(
        &db,
        NewDelivery {
            store_id: None,
            name: Some("PUDO 渋谷".into()),
            lat: Some(35.66),
            lng: Some(139.70),
            carrier: "pudo".into(),
            external_id: Some("pudo-9001".into()),
            can_ship: true,
            can_receive: true,
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(locker.store_id, None);
    assert_eq!(count(&db, delivery::Entity::find()).await.unwrap(), 2);
}

#[tokio::test]
async fn property_unit_listing_chain() {
    let db = db().await;
    let p = upsert_property(
        &db,
        NewProperty {
            name: "サンプルマンション".into(),
            address: "東京都世田谷区1-2-3".into(),
            lat: Some(35.64),
            lng: Some(139.65),
            built_year: Some(2010),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let u = upsert_unit(
        &db,
        NewUnit {
            property_id: p.id,
            dedup_key: "3f|1k|25.5|92000".into(),
            floor: Some("3".into()),
            layout: Some("1K".into()),
            area_m2: Some(25.5),
            rent_yen: Some(92000),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Same room from two sites -> one unit, two listings.
    for site in [ListingSite::Suumo, ListingSite::Homes] {
        record_listing(
            &db,
            NewListing {
                unit_id: Some(u.id),
                site,
                site_listing_id: format!("{site:?}-1"),
                url: "https://example.com/x".into(),
                raw_json: "{}".into(),
            },
        )
        .await
        .unwrap();
    }

    let u2 = upsert_unit(
        &db,
        NewUnit {
            property_id: p.id,
            dedup_key: "3f|1k|25.5|92000".into(),
            rent_yen: Some(90000),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(u.id, u2.id);
    assert_eq!(u2.rent_yen, Some(90000));
    assert_eq!(
        count(&db, chintai_db::entities::unit::Entity::find())
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        count(&db, chintai_db::entities::listing::Entity::find())
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn route_cache_keys_on_rounded_coords() {
    let db = db().await;
    let key = RouteCacheKey {
        from_lat: 35.658034,
        from_lng: 139.701636,
        to_lat: 35.6895,
        to_lng: 139.7004,
        mode: TravelMode::Transit,
    };

    assert!(route_cache_get(&db, &key).await.unwrap().is_none());

    route_cache_put(
        &db,
        &key,
        RouteCacheValue {
            duration_sec: 1320,
            transit_fastest_sec: Some(1200),
            transit_slowest_sec: Some(1620),
            walk_ascent_m: Some(8.0),
            walk_descent_m: Some(3.0),
            provider: RouteProviderKind::GoogleScrape,
            result_json: Some(r#"{"legs":[]}"#.into()),
        },
    )
    .await
    .unwrap();

    // A query that differs only below the 5th decimal must hit the same row.
    let nearby = RouteCacheKey {
        from_lat: 35.6580341,
        from_lng: 139.7016362,
        ..key.clone()
    };
    let hit = route_cache_get(&db, &nearby)
        .await
        .unwrap()
        .expect("cache hit");
    assert_eq!(hit.transit_fastest_sec, Some(1200));

    // Updating the same key replaces, doesn't duplicate.
    route_cache_put(
        &db,
        &key,
        RouteCacheValue {
            duration_sec: 1000,
            transit_fastest_sec: Some(1000),
            transit_slowest_sec: Some(1400),
            walk_ascent_m: None,
            walk_descent_m: None,
            provider: RouteProviderKind::GoogleApi,
            result_json: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(count(&db, route_cache::Entity::find()).await.unwrap(), 1);
    let hit = route_cache_get(&db, &key).await.unwrap().unwrap();
    assert_eq!(hit.duration_sec, 1000);
    assert_eq!(hit.provider, RouteProviderKind::GoogleApi);
}

#[tokio::test]
async fn ingest_run_lifecycle() {
    let db = db().await;
    let id = ingest_run_start(&db, "overpass:13").await.unwrap();
    ingest_run_finish(&db, id, true, Some("2100 stores"))
        .await
        .unwrap();

    let row = chintai_db::entities::ingest_run::Entity::find_by_id(id)
        .one(&db)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(row.ok, Some(true));
    assert!(row.finished_at.is_some());
}
