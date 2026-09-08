//! Shared route data types. Everything is `Serialize`/`Deserialize` so a
//! `RouteResult` round-trips through `route_cache.result_json`.

use std::time::Duration;

use crate::http::HttpError;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LatLng {
    pub lat: f64,
    pub lng: f64,
}

impl LatLng {
    pub fn new(lat: f64, lng: f64) -> Self {
        Self { lat, lng }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Walk,
    Car,
    Transit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    GoogleScrape,
    GoogleApi,
}

#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("no route found")]
    NoRoute,
    #[error("response parse: {0}")]
    Parse(String),
    #[error("provider unavailable: {0}")]
    Unavailable(String),
}

fn ser_dur<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_u64(d.as_secs())
}
fn de_dur<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Duration, D::Error> {
    Ok(Duration::from_secs(u64::deserialize(d)?))
}
fn ser_dur_opt<S: serde::Serializer>(d: &Option<Duration>, s: S) -> Result<S::Ok, S::Error> {
    match d {
        Some(d) => s.serialize_some(&d.as_secs()),
        None => s.serialize_none(),
    }
}
fn de_dur_opt<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Duration>, D::Error> {
    Ok(Option::<u64>::deserialize(d)?.map(Duration::from_secs))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResult {
    pub mode: Mode,
    pub provider: ProviderKind,
    /// Representative door-to-door time (typical departure).
    #[serde(serialize_with = "ser_dur", deserialize_with = "de_dur")]
    pub duration: Duration,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub distance_m: Option<f64>,
    /// Transit only: spread across the alternatives Maps offered.
    #[serde(
        serialize_with = "ser_dur_opt",
        deserialize_with = "de_dur_opt",
        default
    )]
    pub transit_fastest: Option<Duration>,
    #[serde(
        serialize_with = "ser_dur_opt",
        deserialize_with = "de_dur_opt",
        default
    )]
    pub transit_slowest: Option<Duration>,
    /// Walk only (also the walking legs of a transit trip): metres climbed /
    /// descended along the route.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub walk_ascent_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub walk_descent_m: Option<f64>,
    /// One or more concrete itineraries. For walk/car there is exactly one; for
    /// transit these are the alternatives, fastest first.
    #[serde(default)]
    pub itineraries: Vec<Itinerary>,
}

impl RouteResult {
    pub fn single(mode: Mode, provider: ProviderKind, it: Itinerary) -> Self {
        Self {
            mode,
            provider,
            duration: it.duration,
            distance_m: it.distance_m(),
            transit_fastest: None,
            transit_slowest: None,
            walk_ascent_m: None,
            walk_descent_m: None,
            itineraries: vec![it],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Itinerary {
    #[serde(serialize_with = "ser_dur", deserialize_with = "de_dur")]
    pub duration: Duration,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub depart: Option<String>, // "HH:MM" local
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub arrive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub fare_yen: Option<i32>,
    pub transfers: u32,
    #[serde(default)]
    pub legs: Vec<Leg>,
}

impl Itinerary {
    fn distance_m(&self) -> Option<f64> {
        let total: f64 = self.legs.iter().filter_map(|l| l.distance_m()).sum();
        (total > 0.0).then_some(total)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Leg {
    Walk(WalkLeg),
    Transit(TransitLeg),
    Drive(DriveLeg),
}

impl Leg {
    fn distance_m(&self) -> Option<f64> {
        match self {
            Leg::Walk(w) => w.distance_m,
            Leg::Transit(t) => t.distance_m,
            Leg::Drive(d) => d.distance_m,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalkLeg {
    #[serde(serialize_with = "ser_dur", deserialize_with = "de_dur")]
    pub duration: Duration,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub distance_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ascent_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub descent_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub polyline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveLeg {
    #[serde(serialize_with = "ser_dur", deserialize_with = "de_dur")]
    pub duration: Duration,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub distance_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub polyline: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitLeg {
    #[serde(serialize_with = "ser_dur", deserialize_with = "de_dur")]
    pub duration: Duration,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub distance_m: Option<f64>,
    /// "山手線", "東急東横線" …
    pub line_name: String,
    /// "JR東日本", "東京メトロ", "東急電鉄" …
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub operator: Option<String>,
    /// "各駅停車" / "快速" / "特急" / "通勤急行" …
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub train_type: Option<String>,
    /// train / subway / bus / tram …
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub vehicle: Option<String>,
    /// "渋谷行" のような方面表示。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub headsign: Option<String>,
    pub from_station: String,
    pub to_station: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub depart: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub arrive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub num_stops: Option<u32>,
    /// Colour Maps uses for this line, "#RRGGBB", handy for the UI.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub color: Option<String>,
}
