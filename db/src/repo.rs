//! Repository helpers: the small, intention-revealing operations the scraper and
//! server actually call. Everything is an upsert keyed on the same unique index
//! the migration defines, so re-crawls converge instead of duplicating.
//!
//! Each upsert is `INSERT ... ON CONFLICT DO UPDATE` (atomic, race-safe)
//! followed by a `SELECT` on the natural key to return the row. We deliberately
//! do **not** use `exec_with_returning` here: on SQLite `last_insert_rowid()` is
//! connection-global, so after an interleaved insert on another table a
//! conflict-update would resolve the wrong row (or none).

use chrono::Utc;
use sea_orm::{
    sea_query::OnConflict, ActiveValue::Set, ColumnTrait, DatabaseConnection, DbErr, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Select,
};

use crate::entities::enums::{
    BrandKind, ListingSite, PriceTier, RouteProviderKind, StoreSource, TravelMode,
};
use crate::entities::{
    brand, delivery, ingest_run, listing, property, railway_line, route_cache, station,
    station_line, store, unit,
};

pub use sea_orm::prelude::DateTimeUtc;

fn now() -> DateTimeUtc {
    Utc::now()
}

fn missing(what: &str) -> DbErr {
    DbErr::RecordNotFound(format!("{what} vanished immediately after upsert"))
}

/// Coordinates are rounded to 5 decimals (~1 m) before they become a cache key
/// so "the same trip" actually hits the cache.
pub fn round_coord(v: f64) -> f64 {
    (v * 100_000.0).round() / 100_000.0
}

// --------------------------------------------------------------------------
// brand / store / delivery
// --------------------------------------------------------------------------

pub async fn upsert_brand(
    db: &DatabaseConnection,
    name: &str,
    kind: BrandKind,
    price_tier: PriceTier,
) -> Result<brand::Model, DbErr> {
    brand::Entity::insert(brand::ActiveModel {
        name: Set(name.to_owned()),
        kind: Set(kind),
        price_tier: Set(price_tier),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(brand::Column::Name)
            .update_columns([brand::Column::Kind, brand::Column::PriceTier])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    brand::Entity::find()
        .filter(brand::Column::Name.eq(name))
        .one(db)
        .await?
        .ok_or_else(|| missing("brand"))
}

#[derive(Debug, Clone, Default)]
pub struct NewStore {
    pub brand_id: i32,
    pub name: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub address: Option<String>,
    pub source: Option<StoreSource>,
    pub external_id: Option<String>,
}

pub async fn upsert_store(db: &DatabaseConnection, s: NewStore) -> Result<store::Model, DbErr> {
    let source = s.source.unwrap_or(StoreSource::BrandSite);
    let external_id = s.external_id.clone();

    store::Entity::insert(store::ActiveModel {
        brand_id: Set(s.brand_id),
        name: Set(s.name),
        lat: Set(s.lat),
        lng: Set(s.lng),
        address: Set(s.address),
        source: Set(source),
        external_id: Set(external_id.clone()),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([store::Column::Source, store::Column::ExternalId])
            .update_columns([
                store::Column::BrandId,
                store::Column::Name,
                store::Column::Lat,
                store::Column::Lng,
                store::Column::Address,
                store::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    // With no external id there is no natural key: the row we just inserted is
    // the newest one for this (source) with a NULL external id.
    let mut q = store::Entity::find().filter(store::Column::Source.eq(source));
    q = match &external_id {
        Some(id) => q.filter(store::Column::ExternalId.eq(id.clone())),
        None => q.filter(store::Column::ExternalId.is_null()),
    };
    q.order_by_desc(store::Column::Id)
        .one(db)
        .await?
        .ok_or_else(|| missing("store"))
}

#[derive(Debug, Clone, Default)]
pub struct NewDelivery {
    pub store_id: Option<i32>,
    pub name: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub address: Option<String>,
    pub carrier: String,
    pub external_id: Option<String>,
    pub can_ship: bool,
    pub can_receive: bool,
}

pub async fn upsert_delivery(
    db: &DatabaseConnection,
    d: NewDelivery,
) -> Result<delivery::Model, DbErr> {
    let carrier = d.carrier.clone();
    let external_id = d.external_id.clone();

    delivery::Entity::insert(delivery::ActiveModel {
        store_id: Set(d.store_id),
        name: Set(d.name),
        lat: Set(d.lat),
        lng: Set(d.lng),
        address: Set(d.address),
        carrier: Set(d.carrier),
        external_id: Set(external_id.clone()),
        can_ship: Set(d.can_ship),
        can_receive: Set(d.can_receive),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([delivery::Column::Carrier, delivery::Column::ExternalId])
            .update_columns([
                delivery::Column::StoreId,
                delivery::Column::Name,
                delivery::Column::Lat,
                delivery::Column::Lng,
                delivery::Column::Address,
                delivery::Column::CanShip,
                delivery::Column::CanReceive,
                delivery::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    let mut q = delivery::Entity::find().filter(delivery::Column::Carrier.eq(carrier));
    q = match &external_id {
        Some(id) => q.filter(delivery::Column::ExternalId.eq(id.clone())),
        None => q.filter(delivery::Column::ExternalId.is_null()),
    };
    q.order_by_desc(delivery::Column::Id)
        .one(db)
        .await?
        .ok_or_else(|| missing("delivery"))
}

// --------------------------------------------------------------------------
// property / unit / listing
// --------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct NewProperty {
    pub name: String,
    pub address: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub building_type: Option<String>,
    pub built_year: Option<i32>,
    pub structure: Option<String>,
    /// Station name as written in the listing (e.g. "三軒茶屋"). Set once.
    pub station_name: Option<String>,
}

pub async fn upsert_property(
    db: &DatabaseConnection,
    p: NewProperty,
) -> Result<property::Model, DbErr> {
    let (name, address) = (p.name.clone(), p.address.clone());

    property::Entity::insert(property::ActiveModel {
        name: Set(p.name),
        address: Set(p.address),
        lat: Set(p.lat),
        lng: Set(p.lng),
        building_type: Set(p.building_type),
        built_year: Set(p.built_year),
        structure: Set(p.structure),
        station_name: Set(p.station_name),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([property::Column::Name, property::Column::Address])
            // `lat`/`lng`/`station_name` are filled once (by the coordinate
            // heuristic / geocoder / this insert) and deliberately excluded
            // here: a rental re-crawl always passes `lat: None` on this path,
            // and including them would null out coordinates set afterward by
            // `set_property_coords`. `station_walk_min`/`station_distance_m`/
            // `station_car_min`/`station_lat`/`station_lng` are never touched
            // by this function at all (see `set_property_station_time`).
            .update_columns([
                property::Column::BuildingType,
                property::Column::BuiltYear,
                property::Column::Structure,
                property::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    property::Entity::find()
        .filter(property::Column::Name.eq(name))
        .filter(property::Column::Address.eq(address))
        .one(db)
        .await?
        .ok_or_else(|| missing("property"))
}

#[derive(Debug, Clone, Default)]
pub struct NewUnit {
    pub property_id: i32,
    pub dedup_key: String,
    pub floor: Option<String>,
    pub layout: Option<String>,
    pub area_m2: Option<f64>,
    pub rent_yen: Option<i32>,
    pub admin_fee_yen: Option<i32>,
    pub deposit_yen: Option<i32>,
    pub key_money_yen: Option<i32>,
    pub available_from: Option<String>,
}

pub async fn upsert_unit(db: &DatabaseConnection, u: NewUnit) -> Result<unit::Model, DbErr> {
    let (property_id, dedup_key) = (u.property_id, u.dedup_key.clone());

    unit::Entity::insert(unit::ActiveModel {
        property_id: Set(u.property_id),
        dedup_key: Set(u.dedup_key),
        floor: Set(u.floor),
        layout: Set(u.layout),
        area_m2: Set(u.area_m2),
        rent_yen: Set(u.rent_yen),
        admin_fee_yen: Set(u.admin_fee_yen),
        deposit_yen: Set(u.deposit_yen),
        key_money_yen: Set(u.key_money_yen),
        available_from: Set(u.available_from),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([unit::Column::PropertyId, unit::Column::DedupKey])
            .update_columns([
                unit::Column::Floor,
                unit::Column::Layout,
                unit::Column::AreaM2,
                unit::Column::RentYen,
                unit::Column::AdminFeeYen,
                unit::Column::DepositYen,
                unit::Column::KeyMoneyYen,
                unit::Column::AvailableFrom,
                unit::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    unit::Entity::find()
        .filter(unit::Column::PropertyId.eq(property_id))
        .filter(unit::Column::DedupKey.eq(dedup_key))
        .one(db)
        .await?
        .ok_or_else(|| missing("unit"))
}

#[derive(Debug, Clone)]
pub struct NewListing {
    pub unit_id: Option<i32>,
    pub site: ListingSite,
    pub site_listing_id: String,
    pub url: String,
    pub raw_json: String,
}

/// Insert a listing or bump `last_seen` (+ url / raw_json / unit link) if the
/// `(site, site_listing_id)` pair was already recorded.
pub async fn record_listing(
    db: &DatabaseConnection,
    l: NewListing,
) -> Result<listing::Model, DbErr> {
    let (site, site_listing_id) = (l.site, l.site_listing_id.clone());
    let ts = now();

    listing::Entity::insert(listing::ActiveModel {
        unit_id: Set(l.unit_id),
        site: Set(l.site),
        site_listing_id: Set(l.site_listing_id),
        url: Set(l.url),
        first_seen: Set(ts),
        last_seen: Set(ts),
        raw_json: Set(l.raw_json),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([listing::Column::Site, listing::Column::SiteListingId])
            .update_columns([
                listing::Column::UnitId,
                listing::Column::Url,
                listing::Column::LastSeen,
                listing::Column::RawJson,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    listing::Entity::find()
        .filter(listing::Column::Site.eq(site))
        .filter(listing::Column::SiteListingId.eq(site_listing_id))
        .one(db)
        .await?
        .ok_or_else(|| missing("listing"))
}

/// Properties with no coordinates yet (the rental scrapers store an address
/// only; a geocoding pass fills these in for the ranking engine).
pub async fn properties_missing_coords(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<property::Model>, DbErr> {
    property::Entity::find()
        .filter(property::Column::Lat.is_null())
        .order_by_asc(property::Column::Id)
        .limit(limit)
        .all(db)
        .await
}

pub async fn set_property_coords(
    db: &DatabaseConnection,
    id: i32,
    lat: f64,
    lng: f64,
) -> Result<(), DbErr> {
    property::Entity::update(property::ActiveModel {
        id: Set(id),
        lat: Set(Some(lat)),
        lng: Set(Some(lng)),
        updated_at: Set(now()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
}

/// Properties that have a listing station name but no Google-walked time yet —
/// the queue for `ingest google-stations`. Needs coordinates too, obviously.
pub async fn properties_needing_station_time(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<property::Model>, DbErr> {
    property::Entity::find()
        .filter(property::Column::StationName.is_not_null())
        .filter(property::Column::StationName.ne(""))
        .filter(property::Column::StationWalkMin.is_null())
        .filter(property::Column::Lat.is_not_null())
        .order_by_asc(property::Column::Id)
        .limit(limit)
        .all(db)
        .await
}

/// Properties whose walk leg resolved *before* car-time / station-coordinate
/// support existed (so `station_walk_min` is a real, non-negative value but
/// `station_car_min` is still untouched) — the catch-up queue so an
/// already-running crawl doesn't leave a permanent backlog when this feature
/// lands mid-crawl.
pub async fn properties_needing_station_car_backfill(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<property::Model>, DbErr> {
    property::Entity::find()
        .filter(property::Column::StationName.is_not_null())
        .filter(property::Column::StationName.ne(""))
        .filter(property::Column::StationWalkMin.gte(0))
        .filter(property::Column::StationCarMin.is_null())
        .filter(property::Column::Lat.is_not_null())
        .order_by_asc(property::Column::Id)
        .limit(limit)
        .all(db)
        .await
}

/// Fills in just the car leg (+ station coordinates) for a property whose
/// walk leg is already resolved — see `properties_needing_station_car_backfill`.
pub async fn set_property_station_car(
    db: &DatabaseConnection,
    id: i32,
    car_min: Option<i32>,
    station_lat: f64,
    station_lng: f64,
) -> Result<(), DbErr> {
    property::Entity::update(property::ActiveModel {
        id: Set(id),
        station_car_min: Set(car_min),
        station_lat: Set(Some(station_lat)),
        station_lng: Set(Some(station_lng)),
        updated_at: Set(now()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
}

/// `car_min`: `None` if the car leg wasn't attempted (e.g. an older run, or a
/// transient failure — distinct from `Some(-1)`, "attempted and failed").
#[allow(clippy::too_many_arguments)]
pub async fn set_property_station_time(
    db: &DatabaseConnection,
    id: i32,
    walk_min: i32,
    distance_m: Option<f64>,
    car_min: Option<i32>,
    station_lat: f64,
    station_lng: f64,
) -> Result<(), DbErr> {
    property::Entity::update(property::ActiveModel {
        id: Set(id),
        station_walk_min: Set(Some(walk_min)),
        station_distance_m: Set(distance_m),
        station_car_min: Set(car_min),
        station_lat: Set(Some(station_lat)),
        station_lng: Set(Some(station_lng)),
        updated_at: Set(now()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
}

/// Properties with no `station_name` yet, oldest first. One-time backfill
/// target for rows upserted before that column existed — new rows get it
/// straight from `rental::ingest_area`.
pub async fn properties_without_station_name(
    db: &DatabaseConnection,
    limit: u64,
) -> Result<Vec<property::Model>, DbErr> {
    property::Entity::find()
        .filter(property::Column::StationName.is_null())
        .order_by_asc(property::Column::Id)
        .limit(limit)
        .all(db)
        .await
}

pub async fn units_for_property(
    db: &DatabaseConnection,
    property_id: i32,
) -> Result<Vec<unit::Model>, DbErr> {
    unit::Entity::find()
        .filter(unit::Column::PropertyId.eq(property_id))
        .all(db)
        .await
}

pub async fn set_property_station_name(
    db: &DatabaseConnection,
    id: i32,
    name: &str,
) -> Result<(), DbErr> {
    property::Entity::update(property::ActiveModel {
        id: Set(id),
        station_name: Set(Some(name.to_owned())),
        updated_at: Set(now()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
}

/// Marks a property's station lookup as attempted-but-failed (the station
/// name didn't resolve against `station`), storing `-1` as a sentinel so it
/// doesn't get retried on every batch run. `nearest_station_of` in the engine
/// treats a negative `station_walk_min` as "no station data".
pub async fn mark_property_station_unresolved(
    db: &DatabaseConnection,
    id: i32,
) -> Result<(), DbErr> {
    property::Entity::update(property::ActiveModel {
        id: Set(id),
        station_walk_min: Set(Some(-1)),
        station_car_min: Set(Some(-1)),
        updated_at: Set(now()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
}

// --------------------------------------------------------------------------
// railway_line / station / station_line
// --------------------------------------------------------------------------

pub async fn upsert_railway_line(
    db: &DatabaseConnection,
    name: &str,
    operator: &str,
    kind: Option<&str>,
    osm_id: Option<&str>,
) -> Result<railway_line::Model, DbErr> {
    railway_line::Entity::insert(railway_line::ActiveModel {
        name: Set(name.to_owned()),
        operator: Set(operator.to_owned()),
        kind: Set(kind.map(str::to_owned)),
        osm_id: Set(osm_id.map(str::to_owned)),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([railway_line::Column::Name, railway_line::Column::Operator])
            .update_columns([railway_line::Column::Kind, railway_line::Column::UpdatedAt])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    railway_line::Entity::find()
        .filter(railway_line::Column::Name.eq(name))
        .filter(railway_line::Column::Operator.eq(operator))
        .one(db)
        .await?
        .ok_or_else(|| missing("railway_line"))
}

#[derive(Debug, Clone, Default)]
pub struct NewStation {
    pub name: String,
    pub lat: f64,
    pub lng: f64,
    pub address: Option<String>,
    pub osm_id: Option<String>,
}

pub async fn upsert_station(
    db: &DatabaseConnection,
    s: NewStation,
) -> Result<station::Model, DbErr> {
    let osm_id = s.osm_id.clone();
    let (name, lat, lng) = (s.name.clone(), s.lat, s.lng);

    station::Entity::insert(station::ActiveModel {
        name: Set(s.name),
        lat: Set(s.lat),
        lng: Set(s.lng),
        address: Set(s.address),
        osm_id: Set(osm_id.clone()),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::column(station::Column::OsmId)
            .update_columns([
                station::Column::Name,
                station::Column::Lat,
                station::Column::Lng,
                station::Column::Address,
                station::Column::UpdatedAt,
            ])
            .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    let mut q = station::Entity::find();
    q = match &osm_id {
        Some(id) => q.filter(station::Column::OsmId.eq(id.clone())),
        None => q
            .filter(station::Column::OsmId.is_null())
            .filter(station::Column::Name.eq(name))
            .filter(station::Column::Lat.eq(lat))
            .filter(station::Column::Lng.eq(lng)),
    };
    q.order_by_desc(station::Column::Id)
        .one(db)
        .await?
        .ok_or_else(|| missing("station"))
}

pub async fn upsert_station_line(
    db: &DatabaseConnection,
    station_id: i32,
    line_id: i32,
    sequence: Option<i32>,
    train_types_json: Option<String>,
) -> Result<station_line::Model, DbErr> {
    station_line::Entity::insert(station_line::ActiveModel {
        station_id: Set(station_id),
        line_id: Set(line_id),
        sequence: Set(sequence),
        train_types_json: Set(train_types_json),
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            station_line::Column::StationId,
            station_line::Column::LineId,
        ])
        .update_columns([
            station_line::Column::Sequence,
            station_line::Column::TrainTypesJson,
            station_line::Column::UpdatedAt,
        ])
        .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    station_line::Entity::find()
        .filter(station_line::Column::StationId.eq(station_id))
        .filter(station_line::Column::LineId.eq(line_id))
        .one(db)
        .await?
        .ok_or_else(|| missing("station_line"))
}

// --------------------------------------------------------------------------
// route_cache
// --------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct RouteCacheKey {
    pub from_lat: f64,
    pub from_lng: f64,
    pub to_lat: f64,
    pub to_lng: f64,
    pub mode: TravelMode,
    /// Departure time-of-day bucketed to 30 minutes (minutes since midnight
    /// JST), or `None` for "no departure time requested". See the doc on
    /// `route_cache::Model::departure_bucket`.
    pub departure_bucket: Option<i32>,
}

impl RouteCacheKey {
    pub fn rounded(mut self) -> Self {
        self.from_lat = round_coord(self.from_lat);
        self.from_lng = round_coord(self.from_lng);
        self.to_lat = round_coord(self.to_lat);
        self.to_lng = round_coord(self.to_lng);
        self
    }
}

#[derive(Debug, Clone)]
pub struct RouteCacheValue {
    pub duration_sec: i32,
    pub transit_fastest_sec: Option<i32>,
    pub transit_slowest_sec: Option<i32>,
    pub walk_ascent_m: Option<f64>,
    pub walk_descent_m: Option<f64>,
    pub provider: RouteProviderKind,
    pub result_json: Option<String>,
}

fn route_cache_query(key: &RouteCacheKey) -> Select<route_cache::Entity> {
    let k = key.clone().rounded();
    let q = route_cache::Entity::find()
        .filter(route_cache::Column::FromLat.eq(k.from_lat))
        .filter(route_cache::Column::FromLng.eq(k.from_lng))
        .filter(route_cache::Column::ToLat.eq(k.to_lat))
        .filter(route_cache::Column::ToLng.eq(k.to_lng))
        .filter(route_cache::Column::Mode.eq(k.mode));
    match k.departure_bucket {
        Some(b) => q.filter(route_cache::Column::DepartureBucket.eq(b)),
        None => q.filter(route_cache::Column::DepartureBucket.is_null()),
    }
}

pub async fn route_cache_get(
    db: &DatabaseConnection,
    key: &RouteCacheKey,
) -> Result<Option<route_cache::Model>, DbErr> {
    route_cache_query(key).one(db).await
}

pub async fn route_cache_put(
    db: &DatabaseConnection,
    key: &RouteCacheKey,
    val: RouteCacheValue,
) -> Result<route_cache::Model, DbErr> {
    let k = key.clone().rounded();
    route_cache::Entity::insert(route_cache::ActiveModel {
        from_lat: Set(k.from_lat),
        from_lng: Set(k.from_lng),
        to_lat: Set(k.to_lat),
        to_lng: Set(k.to_lng),
        mode: Set(k.mode),
        departure_bucket: Set(k.departure_bucket),
        duration_sec: Set(val.duration_sec),
        transit_fastest_sec: Set(val.transit_fastest_sec),
        transit_slowest_sec: Set(val.transit_slowest_sec),
        walk_ascent_m: Set(val.walk_ascent_m),
        walk_descent_m: Set(val.walk_descent_m),
        provider: Set(val.provider),
        result_json: Set(val.result_json),
        computed_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([
            route_cache::Column::FromLat,
            route_cache::Column::FromLng,
            route_cache::Column::ToLat,
            route_cache::Column::ToLng,
            route_cache::Column::Mode,
            route_cache::Column::DepartureBucket,
        ])
        .update_columns([
            route_cache::Column::DurationSec,
            route_cache::Column::TransitFastestSec,
            route_cache::Column::TransitSlowestSec,
            route_cache::Column::WalkAscentM,
            route_cache::Column::WalkDescentM,
            route_cache::Column::Provider,
            route_cache::Column::ResultJson,
            route_cache::Column::ComputedAt,
        ])
        .to_owned(),
    )
    .exec_without_returning(db)
    .await?;

    route_cache_query(key)
        .one(db)
        .await?
        .ok_or_else(|| missing("route_cache"))
}

// --------------------------------------------------------------------------
// ingest_run
// --------------------------------------------------------------------------

pub async fn ingest_run_start(db: &DatabaseConnection, task: &str) -> Result<i32, DbErr> {
    let res = ingest_run::Entity::insert(ingest_run::ActiveModel {
        task: Set(task.to_owned()),
        started_at: Set(now()),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(res.last_insert_id)
}

pub async fn ingest_run_finish(
    db: &DatabaseConnection,
    id: i32,
    ok: bool,
    note: Option<&str>,
) -> Result<(), DbErr> {
    ingest_run::Entity::update(ingest_run::ActiveModel {
        id: Set(id),
        finished_at: Set(Some(now())),
        ok: Set(Some(ok)),
        note: Set(note.map(str::to_owned)),
        ..Default::default()
    })
    .exec(db)
    .await?;
    Ok(())
}

// --------------------------------------------------------------------------
// read models for the ranking engine
// --------------------------------------------------------------------------

/// Every unit joined to its (coordinate-bearing) property. The engine filters
/// and ranks in memory — fine at personal scale (thousands of rows).
pub async fn units_with_geocoded_property(
    db: &DatabaseConnection,
) -> Result<Vec<(unit::Model, property::Model)>, DbErr> {
    let rows = unit::Entity::find()
        .find_also_related(property::Entity)
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(u, p)| {
            let p = p?;
            (p.lat.is_some() && p.lng.is_some()).then_some((u, p))
        })
        .collect())
}

/// Every store that has coordinates, paired with its brand.
pub async fn stores_with_brand(
    db: &DatabaseConnection,
) -> Result<Vec<(store::Model, brand::Model)>, DbErr> {
    let rows = store::Entity::find()
        .find_also_related(brand::Entity)
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(s, b)| {
            let b = b?;
            (s.lat.is_some() && s.lng.is_some()).then_some((s, b))
        })
        .collect())
}

pub async fn deliveries_with_coords(
    db: &DatabaseConnection,
) -> Result<Vec<delivery::Model>, DbErr> {
    Ok(delivery::Entity::find()
        .all(db)
        .await?
        .into_iter()
        .filter(|d| d.lat.is_some() && d.lng.is_some())
        .collect())
}

/// (south, west, north, east)
pub type BBox = (f64, f64, f64, f64);

pub async fn stores_in_bbox(
    db: &DatabaseConnection,
    b: BBox,
) -> Result<Vec<(store::Model, brand::Model)>, DbErr> {
    let rows = store::Entity::find()
        .filter(store::Column::Lat.between(b.0, b.2))
        .filter(store::Column::Lng.between(b.1, b.3))
        .find_also_related(brand::Entity)
        .all(db)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(s, br)| Some((s, br?)))
        .collect())
}

pub async fn deliveries_in_bbox(
    db: &DatabaseConnection,
    b: BBox,
) -> Result<Vec<delivery::Model>, DbErr> {
    delivery::Entity::find()
        .filter(delivery::Column::Lat.between(b.0, b.2))
        .filter(delivery::Column::Lng.between(b.1, b.3))
        .all(db)
        .await
}

pub async fn stations_in_bbox(
    db: &DatabaseConnection,
    b: BBox,
) -> Result<Vec<station::Model>, DbErr> {
    station::Entity::find()
        .filter(station::Column::Lat.between(b.0, b.2))
        .filter(station::Column::Lng.between(b.1, b.3))
        .all(db)
        .await
}

pub async fn stations_all(db: &DatabaseConnection) -> Result<Vec<station::Model>, DbErr> {
    station::Entity::find().all(db).await
}

/// Best station match for a name like "三軒茶屋" / "三軒茶屋駅": exact (with and
/// without the trailing 駅), then a prefix match. Used to give a scraped
/// property approximate coordinates without a geocoding round-trip.
pub async fn station_by_name(
    db: &DatabaseConnection,
    name: &str,
) -> Result<Option<station::Model>, DbErr> {
    let bare = name.trim().trim_end_matches('駅');
    for cand in [format!("{bare}駅"), bare.to_string()] {
        if let Some(m) = station::Entity::find()
            .filter(station::Column::Name.eq(cand))
            .one(db)
            .await?
        {
            return Ok(Some(m));
        }
    }
    station::Entity::find()
        .filter(station::Column::Name.starts_with(bare))
        .one(db)
        .await
}

pub async fn listings_for_unit(
    db: &DatabaseConnection,
    unit_id: i32,
) -> Result<Vec<listing::Model>, DbErr> {
    listing::Entity::find()
        .filter(listing::Column::UnitId.eq(unit_id))
        .all(db)
        .await
}

/// Total rows returned by a query — handy for ingest logging and tests, e.g.
/// `count(db, store::Entity::find()).await?`.
pub async fn count<E>(db: &DatabaseConnection, query: Select<E>) -> Result<u64, DbErr>
where
    E: EntityTrait,
    E::Model: Send + Sync,
{
    query.count(db).await
}
