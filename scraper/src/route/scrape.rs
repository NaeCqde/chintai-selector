//! Google Maps directions scrape — `reqwest` only, no browser, **Google Maps
//! only** (per project direction).
//!
//! Two requests:
//!   1. `GET /maps/dir/<from>/<to>/data=!4m2!4m1!3e{mode}` — the HTML embeds a
//!      `/maps/preview/directions?…&pb=…` link with the exact protobuf request
//!      for that mode (so we never hand-build `pb` from scratch).
//!   2. `GET` that link (optionally patched for a departure time first — see
//!      [`patch_departure_time`]) — a `)]}'`-prefixed JSON with the full
//!      itinerary: per-trip duration/distance and, for transit, per-leg
//!      operator / line / train type / stations / times / stop count, plus
//!      the fastest–slowest spread across alternatives.
//!
//! The transit leg fields are pulled by scanning each step's subtree for
//! strings ending in 線 / 駅 and known train-type words, which survives Google's
//! frequent index reshuffles better than fixed paths.

use std::time::Duration;

use chrono::{Duration as ChronoDuration, FixedOffset, NaiveTime, Utc};
use serde_json::Value;

use super::elevation::GsiElevation;
use super::types::*;
use super::RouteProvider;
use crate::HttpClient;

pub struct GoogleMapsScrape {
    http: HttpClient,
    elevation: GsiElevation,
}

impl GoogleMapsScrape {
    pub fn new(http: HttpClient, elevation: GsiElevation) -> Self {
        // Pace is tuned in `crate::pacing` — same knob for the bulk
        // property->station precompute and the live per-search
        // property->place routes the engine fetches on the spot.
        http.set_host_interval("www.google.com", crate::pacing::GOOGLE_MAPS);
        Self { http, elevation }
    }

    async fn directions_json(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        departure: Option<DepartureTime>,
    ) -> Result<Value, RouteError> {
        let e = match mode {
            Mode::Car => 0,
            Mode::Walk => 2,
            // Never actually reaches a provider (the engine decomposes it
            // into two legs first) — Transit is the closest single-mode
            // fallback if it ever did.
            Mode::Transit | Mode::PickupTransit => 3,
        };
        let dir_url = format!(
            "https://www.google.com/maps/dir/{},{}/{},{}/data=!4m2!4m1!3e{e}",
            from.lat, from.lng, to.lat, to.lng
        );
        let html = self
            .http
            .get_text(&dir_url, &[("hl", "ja"), ("gl", "jp")])
            .await?;

        let rel = extract_preview_url(&html)
            .ok_or_else(|| RouteError::Parse("no preview/directions link in dir HTML".into()))?;
        let rel = match departure {
            Some(d) => patch_departure_time(&rel, e, d)
                .ok_or_else(|| RouteError::Parse("couldn't patch departure time into pb".into()))?,
            None => rel,
        };
        let url = format!("https://www.google.com/{rel}");

        let body = self.http.get_text(&url, &[]).await?;
        let json_str = body.strip_prefix(")]}'").unwrap_or(&body).trim_start();
        serde_json::from_str(json_str).map_err(|e| RouteError::Parse(format!("preview json: {e}")))
    }
}

/// Patches a departure time into the auto-generated preview-directions `pb`
/// (which defaults to "depart now" and has no time fields at all). Found by
/// diffing a real "depart at 8:00" URL a user captured from their own
/// browser against our own default-generated one for the same trip: the
/// `!20m5!1e{mode}!2e3!5e2!6b1!14b1` block shrinks to `!20m3!5e2!6b1!14b1`
/// and a new `!19m3!1e0!2e2!3j{ts}` block is inserted right before it.
///
/// `{ts}` is *not* a real Unix timestamp for the intended instant: verified
/// empirically (two otherwise-identical requests, one with the correct UTC
/// timestamp for "8:00 JST" and one treating "8:00" as if it were already
/// UTC) that Google reads the JST wall-clock date/time as literal UTC
/// numerals with no offset applied. So this builds the timestamp the same
/// way: take the next JST occurrence of `d`'s clock time, then treat those
/// same calendar numbers as UTC when computing epoch seconds.
fn patch_departure_time(rel: &str, mode_code: u8, d: DepartureTime) -> Option<String> {
    let from = format!("%2120m5%211e{mode_code}%212e3%215e2%216b1%2114b1");
    if !rel.contains(&from) {
        return None;
    }
    let ts = google_naive_departure_timestamp(d);
    let to = format!("%2119m3%211e0%212e2%213j{ts}%2120m3%215e2%216b1%2114b1");
    Some(rel.replacen(&from, &to, 1))
}

/// See [`patch_departure_time`]'s doc for why this deliberately does *not*
/// convert JST to a true UTC instant.
fn google_naive_departure_timestamp(d: DepartureTime) -> i64 {
    let jst = FixedOffset::east_opt(9 * 3600).expect("valid fixed offset");
    let now_jst = Utc::now().with_timezone(&jst);
    let target_time = NaiveTime::from_hms_opt(d.hour as u32, d.minute as u32, 0)
        .unwrap_or_else(|| NaiveTime::from_hms_opt(12, 0, 0).unwrap());
    let mut date = now_jst.date_naive();
    if now_jst.time() >= target_time {
        date += ChronoDuration::days(1);
    }
    date.and_time(target_time).and_utc().timestamp()
}

#[async_trait::async_trait]
impl RouteProvider for GoogleMapsScrape {
    async fn route(
        &self,
        from: LatLng,
        to: LatLng,
        mode: Mode,
        departure: Option<DepartureTime>,
    ) -> Result<RouteResult, RouteError> {
        let json = self.directions_json(from, to, mode, departure).await?;
        let trips = json
            .pointer("/0/1")
            .and_then(Value::as_array)
            .filter(|t| !t.is_empty())
            .ok_or(RouteError::NoRoute)?;

        match mode {
            Mode::Transit | Mode::PickupTransit => {
                let mut its: Vec<Itinerary> = trips.iter().filter_map(parse_transit_trip).collect();
                if its.is_empty() {
                    return Err(RouteError::NoRoute);
                }
                its.sort_by_key(|i| i.duration);
                let fastest = its.first().map(|i| i.duration).unwrap_or_default();
                let slowest = its.last().map(|i| i.duration).unwrap_or(fastest);
                Ok(RouteResult {
                    mode,
                    provider: ProviderKind::GoogleScrape,
                    duration: fastest,
                    distance_m: trip_distance_m(&trips[0]),
                    transit_fastest: Some(fastest),
                    transit_slowest: Some(slowest),
                    walk_ascent_m: None,
                    walk_descent_m: None,
                    itineraries: its,
                })
            }
            Mode::Walk | Mode::Car => {
                let secs = trip_duration_s(&trips[0]).ok_or(RouteError::NoRoute)?;
                let dur = Duration::from_secs(secs);
                let mut out = RouteResult::single(
                    mode,
                    ProviderKind::GoogleScrape,
                    Itinerary {
                        duration: dur,
                        depart: None,
                        arrive: None,
                        fare_yen: None,
                        transfers: 0,
                        legs: vec![if mode == Mode::Walk {
                            Leg::Walk(WalkLeg {
                                duration: dur,
                                distance_m: trip_distance_m(&trips[0]),
                                ascent_m: None,
                                descent_m: None,
                                polyline: None,
                            })
                        } else {
                            Leg::Drive(DriveLeg {
                                duration: dur,
                                distance_m: trip_distance_m(&trips[0]),
                                polyline: None,
                            })
                        }],
                    },
                );
                out.distance_m = trip_distance_m(&trips[0]);
                if mode == Mode::Walk {
                    let line = straight_line(from, to, 12);
                    if let Some(d) = self.elevation.delta_for_path(&line).await {
                        out.walk_ascent_m = Some(d.ascent_m);
                        out.walk_descent_m = Some(d.descent_m);
                    }
                }
                Ok(out)
            }
        }
    }
}

// --- parsing -----------------------------------------------------------------

fn extract_preview_url(html: &str) -> Option<String> {
    let start = html.find("maps/preview/directions?")?;
    let tail = &html[start..];
    let end = tail.find(['"', '\'', '\\', ' ', '<']).unwrap_or(tail.len());
    Some(tail[..end].replace("&amp;", "&"))
}

fn trip_duration_s(trip: &Value) -> Option<u64> {
    trip.pointer("/0/3/0").and_then(Value::as_u64)
}

fn trip_distance_m(trip: &Value) -> Option<f64> {
    trip.pointer("/0/2/0").and_then(Value::as_f64)
}

fn parse_transit_trip(trip: &Value) -> Option<Itinerary> {
    let total = Duration::from_secs(trip_duration_s(trip)?);
    let steps = trip.pointer("/1/0/1").and_then(Value::as_array)?;

    let mut legs = Vec::new();
    let mut transit_count = 0u32;
    let (mut depart, mut arrive) = (None, None);

    for step in steps {
        let dur_txt = step
            .pointer("/0/3/1")
            .and_then(Value::as_str)
            .unwrap_or("0 分");
        let dur = Duration::from_secs(jp_minutes(dur_txt) * 60);

        let is_transit = step.as_array().map(|a| a.len()).unwrap_or(0) >= 6
            && step.pointer("/0/5/0/2").is_some();

        if !is_transit {
            legs.push(Leg::Walk(WalkLeg {
                duration: dur,
                distance_m: step.pointer("/0/2/1").and_then(Value::as_str).map(parse_m),
                ascent_m: None,
                descent_m: None,
                polyline: None,
            }));
            continue;
        }

        transit_count += 1;
        let dep = step
            .pointer("/0/5/0/2")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let arr = step
            .pointer("/0/5/1/2")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if depart.is_none() {
            depart = dep.clone();
        }
        if arr.is_some() {
            arrive = arr.clone();
        }
        let operator = step
            .pointer("/0/6/4/0/0")
            .and_then(Value::as_str)
            .map(str::to_owned);

        let strings = collect_strings(step);
        let line_name = strings
            .iter()
            .find(|s| s.ends_with('線'))
            .cloned()
            .unwrap_or_else(|| "路線不明".into());
        let train_type = strings
            .iter()
            .find_map(|s| train_type(s))
            .map(str::to_owned);
        let stations: Vec<&String> = strings.iter().filter(|s| s.ends_with('駅')).collect();
        let from_station = stations.first().map(|s| (*s).clone()).unwrap_or_default();
        let to_station = stations.get(1).map(|s| (*s).clone()).unwrap_or_default();
        let num_stops = (stations.len() as u32).saturating_sub(1).max(1).into();

        legs.push(Leg::Transit(TransitLeg {
            duration: dur,
            distance_m: None,
            line_name,
            operator,
            train_type,
            vehicle: Some("train".into()),
            headsign: None,
            from_station,
            to_station,
            depart: dep,
            arrive: arr,
            num_stops,
            color: None,
        }));
    }

    Some(Itinerary {
        duration: total,
        depart,
        arrive,
        fare_yen: None,
        transfers: transit_count.saturating_sub(1),
        legs,
    })
}

fn collect_strings(v: &Value) -> Vec<String> {
    let mut out = Vec::new();
    fn go(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => {
                if !s.is_empty() {
                    out.push(s.clone());
                }
            }
            Value::Array(a) => a.iter().for_each(|x| go(x, out)),
            Value::Object(o) => o.values().for_each(|x| go(x, out)),
            _ => {}
        }
    }
    go(v, &mut out);
    out
}

fn train_type(s: &str) -> Option<&'static str> {
    const TYPES: &[&str] = &[
        "特別快速",
        "通勤特急",
        "通勤急行",
        "通勤快速",
        "区間急行",
        "区間快速",
        "快速急行",
        "特急",
        "急行",
        "準急",
        "快速",
        "各駅停車",
        "普通",
    ];
    TYPES.iter().find(|t| s == **t).copied()
}

/// "26 分" / "1 時間 3 分" -> minutes.
fn jp_minutes(s: &str) -> u64 {
    let digits: Vec<u64> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect();
    if s.contains("時間") {
        digits.first().copied().unwrap_or(0) * 60 + digits.get(1).copied().unwrap_or(0)
    } else {
        digits.first().copied().unwrap_or(0)
    }
}

/// "1.8 km" / "170 m" -> metres.
fn parse_m(s: &str) -> f64 {
    let n: f64 = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .unwrap_or(0.0);
    if s.contains("km") {
        n * 1000.0
    } else {
        n
    }
}

fn straight_line(a: LatLng, b: LatLng, n: usize) -> Vec<LatLng> {
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            LatLng::new(a.lat + (b.lat - a.lat) * t, a.lng + (b.lng - a.lng) * t)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A trimmed slice of the real preview-directions link Google generates
    /// for a "depart now" transit request (`e=3`), captured from a live
    /// request in this project's own testing — same shape the user's
    /// captured "depart at 8:00" URL had before its `20m`/`19m` block.
    const BASELINE_TAIL: &str = "%2117m1%213e1%2120m5%211e3%212e3%215e2%216b1%2114b1%2146m1%211b0";

    #[test]
    fn patch_departure_time_matches_the_real_captured_transformation() {
        // Shape of the transformation a user captured for a real "depart at
        // 8:00 JST" transit search. The `3j<epoch>` is the next 8:00 JST from
        // *now*, so assert the structure, not a frozen timestamp.
        let d = DepartureTime::new(8, 0);
        let patched = patch_departure_time(BASELINE_TAIL, 3, d).expect("pattern must match");

        let marker = "%2119m3%211e0%212e2%213j";
        let at = patched.find(marker).expect("19m3 block inserted");
        let after = &patched[at + marker.len()..];
        let epoch: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
        assert!(epoch.len() >= 10, "epoch digits present, got {epoch:?}");
        let secs: i64 = epoch.parse().unwrap();
        // Within the next 8 days and in the future (next weekday 08:00 JST).
        let now = chrono::Utc::now().timestamp();
        assert!(
            secs > now && secs < now + 8 * 86_400,
            "epoch {secs} vs now {now}"
        );
        assert!(after.starts_with(&format!("{epoch}%2120m3%215e2%216b1%2114b1")));

        // Everything outside the patched block is untouched.
        assert!(patched.starts_with("%2117m1%213e1%21"));
        assert!(patched.ends_with("%2146m1%211b0"));
    }

    #[test]
    fn patch_departure_time_is_mode_specific() {
        // Car (e=0) baseline's 20m block starts with 1e0, not 1e3 — patching
        // with the wrong mode code must not match.
        assert!(patch_departure_time(BASELINE_TAIL, 0, DepartureTime::new(8, 0)).is_none());
    }

    #[test]
    fn google_naive_departure_timestamp_encodes_hhmm_verbatim() {
        // Regardless of which date got picked (today vs tomorrow, which
        // depends on wall-clock "now"), the encoded instant's UTC-numeral
        // time-of-day must equal the requested JST clock time exactly — that
        // "no offset applied" behaviour is the whole point (see
        // `patch_departure_time`'s doc).
        let d = DepartureTime::new(8, 30);
        let ts = google_naive_departure_timestamp(d);
        assert_eq!(ts.rem_euclid(86_400), 8 * 3600 + 30 * 60);
    }

    fn load(name: &str) -> Value {
        let raw = std::fs::read_to_string(format!("tests/fixtures/{name}")).unwrap();
        let s = raw.strip_prefix(")]}'").unwrap_or(&raw).trim_start();
        serde_json::from_str(s).unwrap()
    }

    #[test]
    fn transit_fixture() {
        let json = load("gmaps_transit.json");
        let trips = json.pointer("/0/1").unwrap().as_array().unwrap();
        let its: Vec<_> = trips.iter().filter_map(parse_transit_trip).collect();
        assert!(its.len() >= 3);

        let fastest = its.iter().min_by_key(|i| i.duration).unwrap();
        assert!(fastest.duration.as_secs() > 900 && fastest.duration.as_secs() < 4000);

        let t: Vec<_> = fastest
            .legs
            .iter()
            .filter_map(|l| match l {
                Leg::Transit(t) => Some(t),
                _ => None,
            })
            .collect();
        assert!(!t.is_empty());
        assert!(t.iter().any(|x| x.line_name.ends_with('線')));
        assert!(t.iter().any(|x| x.from_station.ends_with('駅')));
        assert!(t.iter().any(|x| x.operator.is_some()));
        assert!(t.iter().any(|x| x.depart.is_some()));
    }

    #[test]
    fn walk_fixture() {
        let json = load("gmaps_walk.json");
        let trips = json.pointer("/0/1").unwrap().as_array().unwrap();
        assert_eq!(trip_duration_s(&trips[0]), Some(1551));
        assert_eq!(trip_distance_m(&trips[0]), Some(1798.0));
    }

    #[test]
    fn helpers() {
        assert_eq!(jp_minutes("26 分"), 26);
        assert_eq!(jp_minutes("1 時間 3 分"), 63);
        assert_eq!(parse_m("1.8 km"), 1800.0);
        assert_eq!(parse_m("170 m"), 170.0);
        assert_eq!(train_type("急行"), Some("急行"));
        assert_eq!(train_type("東急田園都市線"), None);
    }

    #[test]
    fn extracts_preview_link() {
        let html =
            r#"<link href="/maps/preview/directions?authuser=0&amp;hl=ja&amp;pb=!1m4" rel="x">"#;
        assert_eq!(
            extract_preview_url(html).unwrap(),
            "maps/preview/directions?authuser=0&hl=ja&pb=!1m4"
        );
    }
}
