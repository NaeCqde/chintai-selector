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
    PaginatorTrait, QueryFilter, QueryOrder, Select,
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
        updated_at: Set(now()),
        ..Default::default()
    })
    .on_conflict(
        OnConflict::columns([property::Column::Name, property::Column::Address])
            .update_columns([
                property::Column::Lat,
                property::Column::Lng,
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
    route_cache::Entity::find()
        .filter(route_cache::Column::FromLat.eq(k.from_lat))
        .filter(route_cache::Column::FromLng.eq(k.from_lng))
        .filter(route_cache::Column::ToLat.eq(k.to_lat))
        .filter(route_cache::Column::ToLng.eq(k.to_lng))
        .filter(route_cache::Column::Mode.eq(k.mode))
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

/// Total rows returned by a query — handy for ingest logging and tests, e.g.
/// `count(db, store::Entity::find()).await?`.
pub async fn count<E>(db: &DatabaseConnection, query: Select<E>) -> Result<u64, DbErr>
where
    E: EntityTrait,
    E::Model: Send + Sync,
{
    query.count(db).await
}
