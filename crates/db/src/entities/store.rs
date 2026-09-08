//! `store` — a physical outlet belonging to a `brand`. Each store gets its own
//! id. `external_id` is the chain's own store code when we have it; the unique
//! index is `(source, external_id)` so re-crawls upsert instead of duplicating
//! (SQLite treats multiple NULL `external_id`s as distinct, which is fine —
//! those rows are de-duplicated by coordinates later).

use super::enums::StoreSource;
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "store")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub brand_id: i32,
    pub name: String,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub address: Option<String>,
    pub source: StoreSource,
    pub external_id: Option<String>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::brand::Entity",
        from = "Column::BrandId",
        to = "super::brand::Column::Id"
    )]
    Brand,
    #[sea_orm(has_many = "super::delivery::Entity")]
    Delivery,
}

impl Related<super::brand::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Brand.def()
    }
}

impl Related<super::delivery::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Delivery.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
