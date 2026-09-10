//! `property_station` — every station name written in a property's listing
//! (not just the first one), each with its own real Google Maps walk/car
//! time to that station once `ingest google-stations` reaches it. One
//! property can have several rows here, same shape as `station_line` being
//! a station×line junction table.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "property_station")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub property_id: i32,
    /// The station name exactly as the listing wrote it (not a
    /// coordinate-nearest guess).
    pub station_name: String,
    /// 0-based position in the listing's own station list — kept so "the
    /// listing's first-mentioned station" is still recoverable even before
    /// any time is computed (walk/car both `None` then).
    pub rank: i32,
    /// Real walking time from a Google Maps route, filled in by
    /// `ingest google-stations`. `None` until that batch reaches this row.
    /// `-1` = attempted but the station name didn't resolve in our DB.
    pub walk_min: Option<i32>,
    pub distance_m: Option<f64>,
    /// Real driving time from a Google Maps route, filled in by the same
    /// `ingest google-stations` pass. Used for 送迎モード
    /// (`Mode::PickupTransit`) — someone drives you to this station, then
    /// transit from there. Same `-1` sentinel.
    pub car_min: Option<i32>,
    /// The resolved station's own coordinates, cached here so the engine can
    /// build 送迎モード's second leg (station -> place, live transit) without
    /// a `station` table lookup at search time.
    pub station_lat: Option<f64>,
    pub station_lng: Option<f64>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::property::Entity",
        from = "Column::PropertyId",
        to = "super::property::Column::Id"
    )]
    Property,
}

impl Related<super::property::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Property.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
