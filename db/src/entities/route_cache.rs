//! `route_cache` — a computed A→B route, keyed on rounded coordinates + mode so
//! the ranking engine doesn't re-query the same trip. Scalar columns are for
//! fast filtering; `result_json` holds the full `RouteResult` (walk legs with
//! ascent/descent, transit legs with operator / line / stations / times / stop
//! count, and the fastest..slowest itinerary spread) for the UI's
//! transfer-guidance display.

use super::enums::{RouteProviderKind, TravelMode};
use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "route_cache")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub from_lat: f64,
    pub from_lng: f64,
    pub to_lat: f64,
    pub to_lng: f64,
    pub mode: TravelMode,
    pub duration_sec: i32,
    pub transit_fastest_sec: Option<i32>,
    pub transit_slowest_sec: Option<i32>,
    pub walk_ascent_m: Option<f64>,
    pub walk_descent_m: Option<f64>,
    pub provider: RouteProviderKind,
    #[sea_orm(column_type = "Text", nullable)]
    pub result_json: Option<String>,
    pub computed_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
