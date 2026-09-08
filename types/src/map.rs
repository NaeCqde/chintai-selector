//! DTOs for the map overlay and the geocode box.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoiKind {
    Convenience,
    Supermarket,
    CheapSupermarket,
    Delivery,
    Station,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoiPoint {
    pub kind: PoiKind,
    pub name: String,
    pub lat: f64,
    pub lng: f64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub brand: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Geocoded {
    pub name: String,
    pub address: String,
    pub lat: f64,
    pub lng: f64,
    /// Which backend produced this ("nominatim" / "google_scrape" / "google_api").
    pub source: String,
}
