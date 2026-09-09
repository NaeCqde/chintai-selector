//! chintai-db — SQLite persistence layer for chintai-selector.
//!
//! This crate owns the database schema (SeaORM entities + migrations) and a
//! small set of repository helpers. It deliberately depends on neither the
//! scraper nor the server crate: data flows *into* it from the scraper and is
//! read *out of* it by the server, but it knows about neither.

pub mod entities;
pub mod migration;
mod repo;

pub use repo::*;
pub use sea_orm;
pub use sea_orm::{DatabaseConnection, DbErr};

use sea_orm::{ConnectOptions, ConnectionTrait, Statement};
use sea_orm_migration::MigratorTrait;
use std::time::Duration;

/// Open (creating if missing) the SQLite database at `url` and run migrations.
///
/// `url` is a SeaORM/sqlx connection string, e.g.
/// `sqlite://chintai.sqlite?mode=rwc` for a file or `sqlite::memory:` for tests.
///
/// An in-memory database lives inside a single connection, so the pool is
/// capped at 1 in that case — otherwise migrations would run on one connection
/// and queries would hit fresh, empty ones.
pub async fn connect(url: &str) -> anyhow::Result<DatabaseConnection> {
    let is_memory = url.contains(":memory:");

    let mut opts = ConnectOptions::new(url.to_owned());
    opts.max_connections(if is_memory { 1 } else { 8 })
        .acquire_timeout(Duration::from_secs(10))
        .sqlx_logging(false);

    let db = sea_orm::Database::connect(opts).await?;

    // The default rollback-journal mode fsyncs every single statement — brutal
    // for the ingest binaries, which write one row per HTTP response (a
    // 150k-property backfill went from "unknown, still running" to done in
    // seconds after this). WAL lets readers (the server) and the writer
    // (an ingest pass) run concurrently without blocking each other either.
    if !is_memory {
        let backend = db.get_database_backend();
        for pragma in ["PRAGMA journal_mode=WAL;", "PRAGMA synchronous=NORMAL;"] {
            db.execute(Statement::from_string(backend, pragma)).await?;
        }
    }

    migration::Migrator::up(&db, None).await?;
    Ok(db)
}
