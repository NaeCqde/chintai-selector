//! `unit` — a room / rental offer inside a `property`. `dedup_key` is built by
//! the scraper (floor + layout + area + rent, normalised) so the same room seen
//! on both SUUMO and HOME'S collapses to one row: unique `(property_id, dedup_key)`.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "unit")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub property_id: i32,
    pub dedup_key: String,
    pub floor: Option<String>,
    /// 1K / 2LDK …
    pub layout: Option<String>,
    pub area_m2: Option<f64>,
    pub rent_yen: Option<i32>,
    pub admin_fee_yen: Option<i32>,
    pub deposit_yen: Option<i32>,
    pub key_money_yen: Option<i32>,
    pub available_from: Option<String>,
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
    #[sea_orm(has_many = "super::listing::Entity")]
    Listing,
}

impl Related<super::property::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Property.def()
    }
}

impl Related<super::listing::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Listing.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
