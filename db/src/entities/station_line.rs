//! `station_line` — a station served by a line, plus the station's "rank" on
//! that line: which train types stop here. `train_types_json` is a JSON array of
//! objects `{ "name": "快速", "stops": true, "rank": 2 }` (higher rank = faster
//! service), populated from a curated master and refined from transit-route
//! scrapes. `(station_id, line_id)` is unique.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "station_line")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub station_id: i32,
    pub line_id: i32,
    /// このラインでの停車順（分かる場合）。
    pub sequence: Option<i32>,
    #[sea_orm(column_type = "Text", nullable)]
    pub train_types_json: Option<String>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::station::Entity",
        from = "Column::StationId",
        to = "super::station::Column::Id"
    )]
    Station,
    #[sea_orm(
        belongs_to = "super::railway_line::Entity",
        from = "Column::LineId",
        to = "super::railway_line::Column::Id"
    )]
    Line,
}

impl Related<super::station::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Station.def()
    }
}

impl Related<super::railway_line::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Line.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
