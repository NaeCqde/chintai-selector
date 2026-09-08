//! `listing` — the raw per-site record a `unit` was built from. Kept around so
//! re-crawls can diff against `raw_json` and so de-duplication has both sides to
//! compare. `unit_id` is NULL until name-matching resolves the building/room.

use super::enums::ListingSite;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "listing")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub unit_id: Option<i32>,
    pub site: ListingSite,
    pub site_listing_id: String,
    pub url: String,
    pub first_seen: DateTimeUtc,
    pub last_seen: DateTimeUtc,
    #[sea_orm(column_type = "Text")]
    pub raw_json: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::unit::Entity",
        from = "Column::UnitId",
        to = "super::unit::Column::Id"
    )]
    Unit,
}

impl Related<super::unit::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Unit.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
