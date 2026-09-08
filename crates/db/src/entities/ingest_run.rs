//! `ingest_run` — one execution of the `ingest` binary (or one of its tasks),
//! for observability of the periodic refresh.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "ingest_run")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// `brands` / `overpass` / `google-fill` / `rentals:<area>` / `all` …
    pub task: String,
    pub started_at: DateTimeUtc,
    pub finished_at: Option<DateTimeUtc>,
    pub ok: Option<bool>,
    #[sea_orm(column_type = "Text", nullable)]
    pub note: Option<String>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
