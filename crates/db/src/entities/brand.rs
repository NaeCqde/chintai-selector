//! `brand` — a convenience-store or supermarket chain, with its price tier.
//! Convenience stores are always `Expensive`; supermarkets are tagged from a
//! static master list in the scraper crate.

use super::enums::{BrandKind, PriceTier};
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "brand")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub name: String,
    pub kind: BrandKind,
    pub price_tier: PriceTier,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::store::Entity")]
    Store,
}

impl Related<super::store::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Store.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
