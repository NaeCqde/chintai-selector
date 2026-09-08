//! `station` — a rail station (from OSM `railway=station`, supplemented by
//! transit-route scrapes). Which train types actually stop here is per-line, so
//! it lives on `station_line`, not here.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "station")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    pub lat: f64,
    pub lng: f64,
    pub address: Option<String>,
    #[sea_orm(unique)]
    pub osm_id: Option<String>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::station_line::Entity")]
    StationLine,
}

impl Related<super::station_line::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::StationLine.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
