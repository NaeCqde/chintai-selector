//! String-backed enums shared across entities. Stored as `snake_case` text so
//! the SQLite file stays readable and new variants don't renumber anything.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text", rename_all = "snake_case")]
pub enum BrandKind {
    ConvenienceStore,
    Supermarket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text", rename_all = "snake_case")]
pub enum PriceTier {
    /// 安い系スーパー（業務スーパー / OK / ロピア …）。
    Cheap,
    /// 高い系。コンビニは無条件でこちら。
    Expensive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text", rename_all = "snake_case")]
pub enum StoreSource {
    /// ブランド公式の店舗一覧ページから。
    BrandSite,
    /// OpenStreetMap / Overpass から。
    Osm,
    /// Google Maps 個別検索で座標などを補完。
    Google,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text", rename_all = "snake_case")]
pub enum TravelMode {
    Walk,
    Car,
    Transit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text", rename_all = "snake_case")]
pub enum RouteProviderKind {
    GoogleScrape,
    GoogleApi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, EnumIter, DeriveActiveEnum, Serialize, Deserialize)]
#[sea_orm(rs_type = "String", db_type = "Text", rename_all = "snake_case")]
pub enum ListingSite {
    Suumo,
    Homes,
}
