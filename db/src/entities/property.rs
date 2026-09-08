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
