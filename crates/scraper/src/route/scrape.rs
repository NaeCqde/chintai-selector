//! Google Maps directions scrape — `reqwest` only, no browser.
//!
//! What actually works without a headless browser: hitting `/maps/dir/…` and
//! reading the **durations** back out of the server-rendered HTML
//! (`APP_INITIALIZATION_STATE` plus visible "X分" / "X時間Y分" text). What
//! mostly does *not* survive without executing Maps' JS: the structured
//! per-leg transit detail (line, operator, stations, times). So for `Transit`
//! this backend returns the fastest/slowest totals and little else, and
//! [`super::FallbackProvider`] escalates to the API for the detail. For
//! `Walk`/`Car` a duration is the whole answer, and we add elevation from GSI
//! by sampling the straight line.

use std::time::Duration;

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
        Self { http, elevation }
    }
}

#[async_trait::async_trait]
impl RouteProvider for GoogleMapsScrape {
    async fn route(&self, from: LatLng, to: LatLng, mode: Mode) -> Result<RouteResult, RouteError> {
        let e3 = match mode {
            Mode::Car => "0",
            Mode::Walk => "2",
            Mode::Transit => "3",
        };
        let url = format!(
            "https://www.google.com/maps/dir/{},{}/{},{}/data=!4m2!4m1!3e{}",
            from.lat, from.lng, to.lat, to.lng, e3
        );

        let html = self
            .http
            .get_text(&url, &[("hl", "ja"), ("gl", "jp")])
            .await?;

        let mut durations = extract_durations(&html);
        durations.sort_unstable();
        durations.dedup();
        let durations: Vec<Duration> = durations
            .into_iter()
            .filter(|s| *s >= 30 && *s <= 6 * 3600)
            .map(Duration::from_secs)
            .collect();

        let Some(&fastest) = durations.first() else {
            return Err(RouteError::NoRoute);
        };
        let slowest = *durations.last().unwrap_or(&fastest);

        let mut result = RouteResult {
            mode,
            provider: ProviderKind::GoogleScrape,
            duration: fastest,
            distance_m: None,
            transit_fastest: (mode == Mode::Transit).then_some(fastest),
            transit_slowest: (mode == Mode::Transit).then_some(slowest),
            walk_ascent_m: None,
            walk_descent_m: None,
            itineraries: vec![Itinerary {
                duration: fastest,
                depart: None,
                arrive: None,
                fare_yen: None,
                transfers: 0,
                legs: Vec::new(),
            }],
        };

        if mode == Mode::Walk {
            let line = straight_line(from, to, 12);
            if let Some(delta) = self.elevation.delta_for_path(&line).await {
                result.walk_ascent_m = Some(delta.ascent_m);
                result.walk_descent_m = Some(delta.descent_m);
            }
        }

        Ok(result)
    }
}

/// Collect duration candidates (in seconds) from the Maps HTML: both the
/// numeric `[..,"1234s",..]` style tokens and visible "X時間Y分" / "X分" text.
fn extract_durations(html: &str) -> Vec<u64> {
    let mut out = Vec::new();

    // "<n>s" JSON tokens, e.g. `,"900s",` — Maps uses these for step durations.
    let bytes = html.as_bytes();
    let mut i = 0;
    while let Some(p) = html[i..].find("s\"") {
        let end = i + p; // index of 's'
                         // walk back over digits
        let mut start = end;
        while start > 0 && bytes[start - 1].is_ascii_digit() {
            start -= 1;
        }
        if start < end && bytes.get(start.wrapping_sub(1)) == Some(&b'"') {
            if let Ok(n) = html[start..end].parse::<u64>() {
                out.push(n);
            }
        }
        i = end + 1;
    }

    // Visible Japanese durations. Maps renders these as `[900,"15 分"]` /
    // `[3900,"1 時間 5 分"]`, so look back generously and tolerate spaces.
    for (idx, _) in html.match_indices('分') {
        let start = floor_char_boundary(html, idx.saturating_sub(48));
        if let Some(secs) = parse_jp_duration(&html[start..idx]) {
            out.push(secs);
        }
    }

    out
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Parse a Japanese duration ending just before '分'. Handles "15", "15 分"'s
/// whitespace, "1 時間 5", "1時間5". Returns seconds.
fn parse_jp_duration(seg: &str) -> Option<u64> {
    let tail: String = seg
        .chars()
        .rev()
        .take_while(|c| {
            c.is_ascii_digit() || *c == '時' || *c == '間' || *c == ' ' || *c == '\u{3000}'
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let cleaned: String = tail.chars().filter(|c| !c.is_whitespace()).collect();
    if cleaned.is_empty() {
        return None;
    }
    if let Some((h, m)) = cleaned.split_once("時間") {
        let h: u64 = h.parse().ok()?;
        let m: u64 = m.parse().unwrap_or(0);
        Some(h * 3600 + m * 60)
    } else if let Some(h) = cleaned.strip_suffix("時間") {
        Some(h.parse::<u64>().ok()? * 3600)
    } else {
        Some(cleaned.parse::<u64>().ok()? * 60)
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

    #[test]
    fn reads_second_tokens_and_jp_text() {
        let html = r#"junk,"540s",more ... 徒歩 約 1時間5分 かかります ... "900s" ... 25分"#;
        let mut d = extract_durations(html);
        d.sort_unstable();
        assert!(d.contains(&540));
        assert!(d.contains(&900));
        assert!(d.contains(&(65 * 60)));
        assert!(d.contains(&(25 * 60)));
    }

    #[test]
    fn jp_duration_forms() {
        assert_eq!(parse_jp_duration("所要 2時間3"), Some(2 * 3600 + 3 * 60));
        assert_eq!(parse_jp_duration("およそ 45"), Some(45 * 60));
        assert_eq!(parse_jp_duration("なし"), None);
    }
}
