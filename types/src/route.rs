//! Route data types. Everything is `Serialize`/`Deserialize` so a `RouteResult`
//! round-trips through `route_cache.result_json` and across the API boundary.
//! (`RouteError` stays in the scraper crate — it wraps `reqwest::Error`.)

use std::time::Duration;

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
    /// 送迎モード: someone drives you from the property to its nearest
    /// station (the precomputed `station_car_min` leg — never a live route
    /// call), then public transit from that station onward. Only the engine
    /// (`server/src/engine`) understands this — it is decomposed into the
    /// two legs before anything reaches a [`crate::route`]-provider-style
    /// backend, so no `RouteProvider` implementation ever receives it in
    /// practice; every match on `Mode` still needs an arm for it, and those
    /// treat it like `Transit` as a defensive fallback.
    PickupTransit,
}

impl Mode {
    pub fn label_ja(self) -> &'static str {
        match self {
            Mode::Walk => "徒歩",
            Mode::Car => "車",
            Mode::Transit => "公共交通",
            Mode::PickupTransit => "送迎(車)+公共交通",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Instant straight-line estimate — no network, no transfer detail.
    Estimate,
    GoogleScrape,
    GoogleApi,
}

impl ProviderKind {
    pub fn is_estimate(self) -> bool {
        matches!(self, ProviderKind::Estimate)
    }
}

/// A local wall-clock time-of-day (24h) to bias a route search by — transit
/// timetables differ a lot between rush hour and midday. Both backends honour
/// this: the Google Routes API (`GOOGLE_MAPS_API_KEY` set) takes it as an
/// official `departureTime` field, and the Google-Maps-scrape path patches it
/// into the generated preview-directions request — see
/// `chintai_scraper::route::scrape`'s `patch_departure_time` doc for how that
/// was reverse engineered from a real captured "depart at" URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DepartureTime {
    /// 0..=23.
    pub hour: u8,
    /// 0..=59.
    pub minute: u8,
}

impl DepartureTime {
    pub fn new(hour: u8, minute: u8) -> Self {
        Self {
            hour: hour.min(23),
            minute: minute.min(59),
        }
    }

    pub fn hhmm(self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// This time plus `mins` minutes, wrapping across midnight. Used for
    /// 送迎モード's second leg: "depart the station after the drive there
    /// takes `mins` minutes."
    pub fn plus_minutes(self, mins: i64) -> Self {
        let total = (self.hour as i64 * 60 + self.minute as i64 + mins).rem_euclid(24 * 60);
        Self::new((total / 60) as u8, (total % 60) as u8)
    }
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

    /// A placeholder for "we don't have enough data to route this yet" (e.g.
    /// 送迎モード before `ingest google-stations` has resolved a property's
    /// car-to-station leg) — a deliberately huge duration so it always fails
    /// a `max_minutes` check rather than being silently treated as reachable.
    pub fn unresolvable(mode: Mode) -> Self {
        Self::single(
            mode,
            ProviderKind::Estimate,
            Itinerary {
                duration: Duration::from_secs(999_999),
                depart: None,
                arrive: None,
                fare_yen: None,
                transfers: 0,
                legs: Vec::new(),
            },
        )
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
