//! Request / response shapes for the ranking engine. All `Serialize`/
//! `Deserialize` so they cross the Leptos server-function boundary unchanged.

use crate::route::{DepartureTime, Mode, RouteResult};
use serde::{Deserialize, Serialize};

/// What kind of place this is, so the UI can default the route-search
/// departure time sensibly. Purely a UI/default-picking convenience — the
/// engine only looks at `PlaceSpec::departure`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaceKind {
    /// 職場・学校 — defaults the departure time to a weekday commute hour.
    Commute,
    /// その他 — defaults the departure time to midday.
    Other,
}

impl PlaceKind {
    /// Default local departure time-of-day for this kind. User-editable
    /// afterwards; this only picks the starting value.
    pub fn default_departure(self) -> DepartureTime {
        match self {
            PlaceKind::Commute => DepartureTime::new(8, 0),
            PlaceKind::Other => DepartureTime::new(10, 0),
        }
    }

    pub fn label_ja(self) -> &'static str {
        match self {
            PlaceKind::Commute => "職場・学校",
            PlaceKind::Other => "その他",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceSpec {
    pub label: String,
    pub lat: f64,
    pub lng: f64,
    pub mode: Mode,
    /// Hard cap: a property is dropped if this place can't be reached within.
    pub max_minutes: u32,
    #[serde(default = "default_place_kind")]
    pub kind: PlaceKind,
    /// Local departure time-of-day used when computing this place's route.
    /// Defaults from `kind` (see [`PlaceKind::default_departure`]) but is
    /// user-editable per place. See [`DepartureTime`] for which backend
    /// actually honours it.
    #[serde(default = "default_departure")]
    pub departure: DepartureTime,
}

fn default_place_kind() -> PlaceKind {
    PlaceKind::Commute
}
fn default_departure() -> DepartureTime {
    PlaceKind::Commute.default_departure()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filters {
    pub max_rent_yen: Option<i32>,
    pub min_area_m2: Option<f64>,
    /// Allowed layouts ("1K", "1LDK", …); empty = any.
    #[serde(default)]
    pub layouts: Vec<String>,
    /// Building at most this many years old.
    pub max_built_age: Option<i32>,
    pub max_conveni_walk_min: Option<u32>,
    pub max_super_walk_min: Option<u32>,
    /// Require a *cheap*-tier supermarket within `max_super_walk_min`.
    #[serde(default)]
    pub require_cheap_super: bool,
    /// Require a parcel pickup/drop point within this many metres.
    pub max_delivery_m: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankKey {
    /// Sum of every place's travel time.
    AccessTotal,
    /// The single worst place's travel time.
    AccessWorst,
    Rent,
    Area,
    RentPerArea,
    /// Walk distance to the nearest supermarket (cheap tier weighted better).
    NearestSuper,
    /// Walk distance to the nearest convenience store.
    NearestConveni,
    /// Newer building ranks higher.
    BuildYear,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RankSpec {
    pub key: RankKey,
    /// Relative weight; the engine normalises each metric to 0..1 first.
    pub weight: f64,
    /// true = smaller is better (rent, access time), false = larger is better.
    pub ascending: bool,
}

impl RankSpec {
    pub fn new(key: RankKey, weight: f64) -> Self {
        let ascending = !matches!(key, RankKey::Area | RankKey::BuildYear);
        Self {
            key,
            weight,
            ascending,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchRequest {
    pub places: Vec<PlaceSpec>,
    #[serde(default)]
    pub filters: Filters,
    /// Weighted, combined. Empty = rank by `AccessTotal` then `Rent`.
    #[serde(default)]
    pub ranking: Vec<RankSpec>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

// --- results ------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub results: Vec<RankedProperty>,
    /// How many properties passed the static + POI filters.
    pub considered: usize,
    /// Of those, how many were close enough (crow-flies) to be worth routing.
    #[serde(default)]
    pub plausible: usize,
    /// How many candidates got a real Google Maps route check (walking the
    /// ranked list until `limit` properties are confirmed, or the pool runs
    /// out).
    #[serde(default)]
    pub routed: usize,
    /// True when `plausible` exceeded the routing cap, so some candidates
    /// further out were not evaluated.
    #[serde(default)]
    pub capped: bool,
    /// How many were confirmed within every place's time limit by a real
    /// route — i.e. how many are actually listed.
    pub matched: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedProperty {
    pub rank: u32,
    pub score: f64,
    pub property_id: i32,
    pub name: String,
    pub address: String,
    pub lat: f64,
    pub lng: f64,
    pub built_year: Option<i32>,
    pub structure: Option<String>,
    pub best_unit: UnitView,
    pub other_units: Vec<UnitView>,
    pub places: Vec<PlaceAccess>,
    /// The listing's nearest station, with the walk time from Google Maps.
    #[serde(default)]
    pub nearest_station: Option<NearestStation>,
    pub nearest_conveni_m: Option<f64>,
    pub nearest_super_m: Option<f64>,
    pub nearest_cheap_super_m: Option<f64>,
    pub nearest_delivery_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearestStation {
    pub name: String,
    /// Walk time from the property, from a Google Maps route.
    pub walk_minutes: u32,
    pub distance_m: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitView {
    pub unit_id: i32,
    pub floor: Option<String>,
    pub layout: Option<String>,
    pub area_m2: Option<f64>,
    pub rent_yen: Option<i32>,
    pub admin_fee_yen: Option<i32>,
    pub deposit_yen: Option<i32>,
    pub key_money_yen: Option<i32>,
    /// Every site this exact unit was found on, with its URL.
    pub sources: Vec<UnitSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitSource {
    pub site: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceAccess {
    pub label: String,
    pub minutes: u32,
    pub within_limit: bool,
    /// Full route detail (transit legs with operator / line / stations / times,
    /// walk ascent/descent, fastest–slowest spread) for the UI's
    /// transfer-guidance panel.
    pub route: RouteResult,
}
