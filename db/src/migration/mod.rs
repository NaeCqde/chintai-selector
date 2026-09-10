//! Schema migrations. The `Migrator` is what `chintai_db::connect` runs on
//! startup and what `sea-orm-migration` drives in tests.

use sea_orm_migration::prelude::*;

mod m20260908_000001_init;
mod m20260910_000002_property_station;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20260908_000001_init::Migration),
            Box::new(m20260910_000002_property_station::Migration),
        ]
    }
}
