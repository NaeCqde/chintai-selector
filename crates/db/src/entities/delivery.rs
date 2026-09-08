//! `delivery` — a package logistics point: whether you can send from it and/or
//! pick up at it. When it *is* a store counter, `store_id` points at the store
//! and the coordinates are read from there. Non-store points (PUDO lockers,
//! carrier depots) leave `store_id` NULL and carry their own coordinates.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "delivery")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub store_id: Option<i32>,
    pub name: Option<String>,
    pub lat: Option<f64>,
    pub lng: Option<f64>,
    pub address: Option<String>,
    /// `yamato` / `sagawa` / `japanpost` / `pudo` … (free text; many carriers).
    pub carrier: String,
    pub external_id: Option<String>,
    pub can_ship: bool,
    pub can_receive: bool,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::store::Entity",
        from = "Column::StoreId",
        to = "super::store::Column::Id"
    )]
    Store,
}

impl Related<super::store::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Store.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
