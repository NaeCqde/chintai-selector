//! Google Routes API (`routes.googleapis.com/directions/v2:computeRoutes`).
//! Needs `GOOGLE_MAPS_API_KEY`. Returns full structured detail; this is the
//! reliable backend the scrape falls back to.
//!
//! Note: the API exposes the transit *line* (name, agency, colour, vehicle
//! type, stop count) but not always the Japanese *train type* ("快速" / "特急").
//! We surface it when it can be read off the headsign / line name and leave it
//! `None` otherwise.

use std::time::Duration;

use super::elevation::GsiElevation;
use super::types::*;
use super::RouteProvider;
use crate::HttpClient;

const URL: &str = "https://routes.googleapis.com/directions/v2:computeRoutes";
const FIELD_MASK: &str = "routes.duration,routes.distanceMeters,routes.polyline.encodedPolyline,\
routes.legs.steps.travelMode,routes.legs.steps.distanceMeters,routes.legs.steps.staticDuration,\
routes.legs.steps.polyline.encodedPolyline,routes.legs.steps.transitDetails";

pub struct GoogleRoutesApi {
    http: HttpClient,
    api_key: String,
    elevation: GsiElevation,
}

impl GoogleRoutesApi {
    pub fn new(http: HttpClient, api_key: String, elevation: GsiElevation) -> Self {
        Self {
            http,
            api_key,
            elevation,
        }
    }
}

#[async_trait::async_trait]
impl RouteProvider for GoogleRoutesApi {
    async fn route(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        departure: Option<DepartureTime>,
    ) -> Result<RouteResult, RouteError> {
        let travel_mode = match mode {
            Mode::Walk => "WALK",
            Mode::Car => "DRIVE",
            Mode::Transit | Mode::PickupTransit => "TRANSIT",
        };
        let mut body = serde_json::json!({
            "origin": { "location": { "latLng": { "latitude": from.lat, "longitude": from.lng } } },
            "destination": { "location": { "latLng": { "latitude": to.lat, "longitude": to.lng } } },
            "travelMode": travel_mode,
            "computeAlternativeRoutes": matches!(mode, Mode::Transit | Mode::PickupTransit),
            "languageCode": "ja",
            "regionCode": "JP",
            "units": "METRIC",
        });
        // Officially documented (unlike the scrape path — see
        // `GoogleMapsScrape`'s doc comment): the Routes API takes an RFC3339
        // departure time directly. "Next occurrence of this JST wall-clock
        // time" — no weekday/holiday awareness, which is a known
        // simplification.
        if let Some(d) = departure {
            body["departureTime"] = serde_json::Value::String(next_departure_rfc3339(d));
        }

        let resp: RoutesResponse = self
            .http
            .post_json(
                URL,
                &[
                    ("X-Goog-Api-Key", self.api_key.as_str()),
                    ("X-Goog-FieldMask", FIELD_MASK),
                ],
                &body,
            )
            .await?;

        if resp.routes.is_empty() {
            return Err(RouteError::NoRoute);
        }

        let mut itineraries: Vec<Itinerary> = Vec::new();
        for r in &resp.routes {
            itineraries.push(self.to_itinerary(r).await);
        }
        itineraries.sort_by_key(|it| it.duration);

        let fastest = itineraries.first().map(|it| it.duration);
        let slowest = itineraries.last().map(|it| it.duration);

        let mut out = RouteResult {
            mode,
            provider: ProviderKind::GoogleApi,
            duration: fastest.unwrap_or_default(),
            distance_m: resp.routes[0].distance_meters.map(|m| m as f64),
            transit_fastest: matches!(mode, Mode::Transit | Mode::PickupTransit)
                .then_some(fastest)
                .flatten(),
            transit_slowest: matches!(mode, Mode::Transit | Mode::PickupTransit)
                .then_some(slowest)
                .flatten(),
            walk_ascent_m: None,
            walk_descent_m: None,
            itineraries,
        };
        self.fill_walk_elevation(&mut out).await;
        Ok(out)
    }
}

impl GoogleRoutesApi {
    async fn to_itinerary(&self, r: &ApiRoute) -> Itinerary {
        let duration = parse_secs(&r.duration);
        let mut legs = Vec::new();
        let (mut depart, mut arrive) = (None, None);

        for leg in &r.legs {
            for step in &leg.steps {
                match step.travel_mode.as_deref() {
                    Some("TRANSIT") => {
                        if let Some(td) = &step.transit_details {
                            if depart.is_none() {
                                depart = td
                                    .stop_details
                                    .as_ref()
                                    .and_then(|s| s.departure_time.as_deref())
                                    .map(hhmm);
                            }
                            if let Some(a) = td
                                .stop_details
                                .as_ref()
                                .and_then(|s| s.arrival_time.as_deref())
                            {
                                arrive = Some(hhmm(a));
                            }
                            legs.push(Leg::Transit(td.to_leg(step)));
                        }
                    }
                    _ => {
                        // WALK (or anything else) — treat as a walk leg.
                        legs.push(Leg::Walk(WalkLeg {
                            duration: parse_secs(step.static_duration.as_deref().unwrap_or("0s")),
                            distance_m: step.distance_meters.map(|m| m as f64),
                            ascent_m: None,
                            descent_m: None,
                            polyline: step.polyline.as_ref().map(|p| p.encoded_polyline.clone()),
                        }));
                    }
                }
            }
        }

        // Fold consecutive walk legs (Routes API emits many short walk steps).
        let legs = fold_walks(legs);
        let transfers =
            (legs.iter().filter(|l| matches!(l, Leg::Transit(_))).count()).saturating_sub(1) as u32;

        Itinerary {
            duration,
            depart,
            arrive,
            fare_yen: None,
            transfers,
            legs,
        }
    }

    async fn fill_walk_elevation(&self, out: &mut RouteResult) {
        // Gather every walk polyline in the (fastest) itinerary and sample it.
        let Some(it) = out.itineraries.first_mut() else {
            return;
        };
        let mut path = Vec::new();
        for leg in &it.legs {
            if let Leg::Walk(w) = leg {
                if let Some(pl) = &w.polyline {
                    path.extend(super::polyline::decode(pl));
                }
            }
        }
        if path.len() < 2 {
            return;
        }
        if let Some(delta) = self.elevation.delta_for_path(&path).await {
            out.walk_ascent_m = Some(delta.ascent_m);
            out.walk_descent_m = Some(delta.descent_m);
        }
    }
}

fn fold_walks(legs: Vec<Leg>) -> Vec<Leg> {
    let mut out: Vec<Leg> = Vec::new();
    for leg in legs {
        if let (Some(Leg::Walk(prev)), Leg::Walk(cur)) = (out.last_mut(), &leg) {
            prev.duration += cur.duration;
            prev.distance_m = Some(prev.distance_m.unwrap_or(0.0) + cur.distance_m.unwrap_or(0.0));
            match (&mut prev.polyline, &cur.polyline) {
                (Some(a), Some(b)) => a.push_str(b),
                (None, Some(b)) => prev.polyline = Some(b.clone()),
                _ => {}
            }
        } else {
            out.push(leg);
        }
    }
    out
}

fn parse_secs(s: &str) -> Duration {
    Duration::from_secs(s.trim_end_matches('s').parse().unwrap_or(0))
}

/// The next occurrence (today if still ahead, otherwise tomorrow) of `d`'s
/// wall-clock time in JST (fixed UTC+9 — Japan has no DST), as an RFC3339 UTC
/// timestamp for the Routes API's `departureTime` field. No weekday/holiday
/// awareness: a "commute" place always means "the next time it's this hour",
/// even if that lands on a weekend.
fn next_departure_rfc3339(d: DepartureTime) -> String {
    use chrono::{Duration as ChronoDuration, FixedOffset, NaiveTime, TimeZone, Utc};
    let jst = FixedOffset::east_opt(9 * 3600).expect("valid fixed offset");
    let now_jst = Utc::now().with_timezone(&jst);
    let target = NaiveTime::from_hms_opt(d.hour as u32, d.minute as u32, 0)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let mut candidate = jst
        .from_local_datetime(&now_jst.date_naive().and_time(target))
        .single()
        .unwrap_or(now_jst);
    if candidate <= now_jst {
        candidate += ChronoDuration::days(1);
    }
    candidate
        .with_timezone(&Utc)
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// "2026-09-08T09:41:00Z" / with offset -> "09:41". Best effort.
fn hhmm(rfc3339: &str) -> String {
    rfc3339
        .split('T')
        .nth(1)
        .map(|t| t.chars().take(5).collect())
        .unwrap_or_else(|| rfc3339.to_owned())
}

// --- wire types -----------------------------------------------------------

#[derive(serde::Deserialize)]
struct RoutesResponse {
    #[serde(default)]
    routes: Vec<ApiRoute>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiRoute {
    #[serde(default)]
    duration: String,
    #[serde(default)]
    distance_meters: Option<i64>,
    #[serde(default)]
    legs: Vec<ApiLeg>,
}

#[derive(serde::Deserialize)]
struct ApiLeg {
    #[serde(default)]
    steps: Vec<ApiStep>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiStep {
    #[serde(default)]
    travel_mode: Option<String>,
    #[serde(default)]
    distance_meters: Option<i64>,
    #[serde(default)]
    static_duration: Option<String>,
    #[serde(default)]
    polyline: Option<ApiPolyline>,
    #[serde(default)]
    transit_details: Option<ApiTransitDetails>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiPolyline {
    #[serde(default)]
    encoded_polyline: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiTransitDetails {
    #[serde(default)]
    stop_details: Option<ApiStopDetails>,
    #[serde(default)]
    headsign: Option<String>,
    #[serde(default)]
    stop_count: Option<u32>,
    #[serde(default)]
    transit_line: Option<ApiTransitLine>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiStopDetails {
    #[serde(default)]
    arrival_stop: Option<ApiNamed>,
    #[serde(default)]
    departure_stop: Option<ApiNamed>,
    #[serde(default)]
    arrival_time: Option<String>,
    #[serde(default)]
    departure_time: Option<String>,
}

#[derive(serde::Deserialize)]
struct ApiNamed {
    #[serde(default)]
    name: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiTransitLine {
    #[serde(default)]
    name: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    agencies: Vec<ApiNamed>,
    #[serde(default)]
    vehicle: Option<ApiVehicle>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiVehicle {
    #[serde(default)]
    r#type: Option<String>,
}

impl ApiTransitDetails {
    fn to_leg(&self, step: &ApiStep) -> TransitLeg {
        let line = self.transit_line.as_ref();
        let (from_station, to_station) = self
            .stop_details
            .as_ref()
            .map(|s| {
                (
                    s.departure_stop
                        .as_ref()
                        .map(|n| n.name.clone())
                        .unwrap_or_default(),
                    s.arrival_stop
                        .as_ref()
                        .map(|n| n.name.clone())
                        .unwrap_or_default(),
                )
            })
            .unwrap_or_default();

        let line_name = line.map(|l| l.name.clone()).unwrap_or_default();
        let train_type = self
            .headsign
            .as_deref()
            .and_then(train_type_from_text)
            .or_else(|| train_type_from_text(&line_name));

        TransitLeg {
            duration: parse_secs(step.static_duration.as_deref().unwrap_or("0s")),
            distance_m: step.distance_meters.map(|m| m as f64),
            line_name,
            operator: line
                .and_then(|l| l.agencies.first())
                .map(|a| a.name.clone())
                .filter(|s| !s.is_empty()),
            train_type,
            vehicle: line
                .and_then(|l| l.vehicle.as_ref())
                .and_then(|v| v.r#type.as_deref())
                .map(vehicle_label),
            headsign: self.headsign.clone().filter(|s| !s.is_empty()),
            from_station,
            to_station,
            depart: self
                .stop_details
                .as_ref()
                .and_then(|s| s.departure_time.as_deref())
                .map(hhmm),
            arrive: self
                .stop_details
                .as_ref()
                .and_then(|s| s.arrival_time.as_deref())
                .map(hhmm),
            num_stops: self.stop_count,
            color: line.and_then(|l| l.color.clone()),
        }
    }
}

fn train_type_from_text(t: &str) -> Option<String> {
    const TYPES: &[&str] = &[
        "特別快速",
        "通勤特急",
        "通勤急行",
        "区間急行",
        "快速急行",
        "特急",
        "急行",
        "準急",
        "快速",
        "各駅停車",
        "普通",
    ];
    TYPES
        .iter()
        .find(|k| t.contains(**k))
        .map(|k| (*k).to_owned())
}

fn vehicle_label(t: &str) -> String {
    match t {
        "BUS" | "INTERCITY_BUS" | "TROLLEYBUS" => "bus",
        "SUBWAY" | "METRO_RAIL" => "subway",
        "TRAM" | "LIGHT_RAIL" | "MONORAIL" => "tram",
        "FERRY" => "ferry",
        _ => "train",
    }
    .to_owned()
}
