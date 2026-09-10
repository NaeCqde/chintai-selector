//! `property_station` — the listing's station name(s) + their real Google Maps
//! walk/car times, moved out of columns on `property` (a listing can name more
//! than one station).
//!
//! For a fresh database this just creates the table. For the pre-existing dev
//! database (150k properties, station data as columns on `property`) it also
//! copies that data across and drops the now-unused `property` columns.

use sea_orm::{ConnectionTrait, Schema, Statement};
use sea_orm_migration::prelude::*;

use crate::entities::property_station;

#[derive(DeriveMigrationName)]
pub struct Migration;

const OLD_COLS: &[&str] = &[
    "station_name",
    "station_walk_min",
    "station_distance_m",
    "station_car_min",
    "station_lat",
    "station_lng",
];

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();
        let db = manager.get_connection();
        let schema = Schema::new(backend);

        manager
            .create_table(
                schema
                    .create_table_from_entity(property_station::Entity)
                    .if_not_exists()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("ux_property_station_name")
                    .table(property_station::Entity)
                    .col(property_station::Column::PropertyId)
                    .col(property_station::Column::StationName)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("ix_property_station_walk")
                    .table(property_station::Entity)
                    .col(property_station::Column::PropertyId)
                    .col(property_station::Column::WalkMin)
                    .to_owned(),
            )
            .await?;

        // --- migrate the pre-existing dev DB --------------------------------
        let has_old = column_exists(db, backend, "property", "station_name").await?;
        if has_old {
            db.execute(Statement::from_string(
                backend,
                "INSERT OR IGNORE INTO property_station \
                   (property_id, station_name, rank, walk_min, distance_m, car_min, \
                    station_lat, station_lng, updated_at) \
                 SELECT id, station_name, 0, station_walk_min, station_distance_m, \
                        station_car_min, station_lat, station_lng, updated_at \
                 FROM property \
                 WHERE station_name IS NOT NULL AND station_name <> ''",
            ))
            .await?;

            for col in OLD_COLS {
                // SQLite 3.35+ supports DROP COLUMN; if it can't, the column
                // just lingers unused (the entity no longer maps it).
                let _ = db
                    .execute(Statement::from_string(
                        backend,
                        format!("ALTER TABLE property DROP COLUMN {col}"),
                    ))
                    .await;
            }
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .table(property_station::Entity)
                    .if_exists()
                    .to_owned(),
            )
            .await
    }
}

async fn column_exists(
    db: &impl ConnectionTrait,
    backend: sea_orm::DatabaseBackend,
    table: &str,
    column: &str,
) -> Result<bool, DbErr> {
    let rows = db
        .query_all(Statement::from_string(
            backend,
            format!("PRAGMA table_info({table})"),
        ))
        .await?;
    for r in rows {
        if let Ok(name) = r.try_get::<String>("", "name") {
            if name == column {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
