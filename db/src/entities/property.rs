//! `property` — a building, after de-duplication across listing sites. The
//! `(name, address)` pair is unique; the scraper resolves fuzzy matches to one
//! canonical building before upserting.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "property")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub address: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    /// マンション / アパート / 一戸建て …
    pub building_type: Option<String>,
    pub built_year: Option<i32>,
    /// RC / SRC / 木造 …
    pub structure: Option<String>,
    /// The station name as written in the rental listing (e.g. "三軒茶屋"),
    /// not a coordinate-nearest guess. Set once at ingest; never overwritten.
    pub station_name: Option<String>,
    /// Real walking time from a Google Maps route, filled in by
    /// `ingest google-stations`. `None` until that batch reaches this row.
    pub station_walk_min: Option<i32>,
    pub station_distance_m: Option<f64>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::unit::Entity")]
    Unit,
}

impl Related<super::unit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Unit.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
