//! chintai-db — SQLite persistence layer for chintai-selector.
//!
//! This crate owns the database schema (SeaORM entities + migrations) and a
//! small set of repository helpers. It deliberately depends on neither the
//! scraper nor the server crate: data flows *into* it from the scraper and is
//! read *out of* it by the server, but it knows about neither.
//!
//! Schema and repository helpers land in the next milestone; this file is the
//! crate's public entry point.

pub use sea_orm::{DatabaseConnection, DbErr};
