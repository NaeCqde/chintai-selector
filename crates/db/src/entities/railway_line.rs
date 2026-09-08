//! `railway_line` — one rail line run by one operator (JR / 私鉄 / 地下鉄 …).
//! `(name, operator)` is unique so "山手線 / JR東日本" and a same-named private
//! line stay distinct.

use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "railway_line")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub name: String,
    /// 運行会社（例: JR東日本 / 東京メトロ / 小田急電鉄）。
    pub operator: String,
    /// jr / subway / private / tram … 任意の分類。
    pub kind: Option<String>,
    pub osm_id: Option<String>,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::station_line::Entity")]
    StationLine,
}

impl Related<super::station_line::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::StationLine.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
