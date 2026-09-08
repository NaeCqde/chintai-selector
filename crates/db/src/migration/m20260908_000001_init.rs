//! Initial schema. Tables are generated from the SeaORM entities
//! (`create_table_from_entity`, which carries over column types, nullability,
//! and single-column `#[sea_orm(unique)]` constraints); composite unique
//! indexes and lookup indexes are added explicitly below.

use sea_orm::{sea_query::Index, Schema};
use sea_orm_migration::prelude::*;

use crate::entities::{
    brand, delivery, ingest_run, listing, property, railway_line, route_cache, station,
    station_line, store, unit,
};

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let schema = Schema::new(manager.get_database_backend());

        macro_rules! create {
            ($entity:expr) => {
                manager
                    .create_table(
                        schema
                            .create_table_from_entity($entity)
                            .if_not_exists()
                            .to_owned(),
                    )
                    .await?;
            };
        }

        // Parents before children (foreign keys).
        create!(brand::Entity);
        create!(store::Entity);
        create!(delivery::Entity);
        create!(property::Entity);
        create!(unit::Entity);
        create!(listing::Entity);
        create!(railway_line::Entity);
        create!(station::Entity);
        create!(station_line::Entity);
        create!(route_cache::Entity);
        create!(ingest_run::Entity);

        macro_rules! index {
            ($name:literal, $entity:expr, [$($col:expr),+ $(,)?], unique) => {
                manager
                    .create_index(
                        Index::create()
                            .if_not_exists()
                            .name($name)
                            .table($entity)
                            $(.col($col))+
                            .unique()
                            .to_owned(),
                    )
                    .await?;
            };
            ($name:literal, $entity:expr, [$($col:expr),+ $(,)?]) => {
                manager
                    .create_index(
                        Index::create()
                            .if_not_exists()
                            .name($name)
                            .table($entity)
                            $(.col($col))+
                            .to_owned(),
                    )
                    .await?;
            };
        }

        index!(
            "ux_store_source_external",
            store::Entity,
            [store::Column::Source, store::Column::ExternalId],
            unique
        );
        index!("ix_store_brand", store::Entity, [store::Column::BrandId]);
        index!(
            "ix_store_lat_lng",
            store::Entity,
            [store::Column::Lat, store::Column::Lng]
        );

        index!(
            "ux_delivery_carrier_external",
            delivery::Entity,
            [delivery::Column::Carrier, delivery::Column::ExternalId],
            unique
        );
        index!(
            "ix_delivery_store",
            delivery::Entity,
            [delivery::Column::StoreId]
        );

        index!(
            "ux_property_name_address",
            property::Entity,
            [property::Column::Name, property::Column::Address],
            unique
        );
        index!(
            "ix_property_lat_lng",
            property::Entity,
            [property::Column::Lat, property::Column::Lng]
        );

        index!(
            "ux_unit_property_dedup",
            unit::Entity,
            [unit::Column::PropertyId, unit::Column::DedupKey],
            unique
        );

        index!(
            "ux_listing_site_id",
            listing::Entity,
            [listing::Column::Site, listing::Column::SiteListingId],
            unique
        );
        index!(
            "ix_listing_unit",
            listing::Entity,
            [listing::Column::UnitId]
        );

        index!(
            "ux_line_name_operator",
            railway_line::Entity,
            [railway_line::Column::Name, railway_line::Column::Operator],
            unique
        );

        index!(
            "ix_station_lat_lng",
            station::Entity,
            [station::Column::Lat, station::Column::Lng]
        );

        index!(
            "ux_station_line",
            station_line::Entity,
            [
                station_line::Column::StationId,
                station_line::Column::LineId
            ],
            unique
        );

        index!(
            "ux_route_cache_key",
            route_cache::Entity,
            [
                route_cache::Column::FromLat,
                route_cache::Column::FromLng,
                route_cache::Column::ToLat,
                route_cache::Column::ToLng,
                route_cache::Column::Mode,
            ],
            unique
        );

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        macro_rules! drop_table {
            ($entity:expr) => {
                manager
                    .drop_table(Table::drop().table($entity).if_exists().to_owned())
                    .await?;
            };
        }

        // Children before parents.
        drop_table!(ingest_run::Entity);
        drop_table!(route_cache::Entity);
        drop_table!(station_line::Entity);
        drop_table!(station::Entity);
        drop_table!(railway_line::Entity);
        drop_table!(listing::Entity);
        drop_table!(unit::Entity);
        drop_table!(property::Entity);
        drop_table!(delivery::Entity);
        drop_table!(store::Entity);
        drop_table!(brand::Entity);

        Ok(())
    }
}
