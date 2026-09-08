//! Nationwide POI collection: convenience stores, supermarkets, parcel /
//! delivery points and rail stations.
//!
//! Sources, in priority order (per project direction):
//!   1. Each chain's official store-list pages (prefecture → city → list).
//!      These are the "list, not search form" pages that can be crawled without
//!      loading the site. Adapters live in `brands/` — a trait plus concrete
//!      chains; a chain with no such page falls through to (2).
//!   2. OpenStreetMap Overpass — `overpass.rs`. One implementation, nationwide
//!      coverage, tiled by prefecture bbox.
//!   3. Google Maps per-store lookup to fill missing coordinates — `google_fill`
//!      (later).
//!
//! The supermarket price-tier master and OSM tag → canonical chain
//! normalisation are in `brand_master.rs`.

pub mod brand_master;
pub mod overpass;
pub mod prefectures;

pub use overpass::{IngestCounts, Overpass, OverpassError};
