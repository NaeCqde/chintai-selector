//! Geocoding: "建物名や住所" -> `{ name, address, lat, lng }`.
//!
//! Several backends behind one trait, tried in order by [`FallbackGeocoder`]:
//!   1. Nominatim (OpenStreetMap) — free, reliable, Japan-scoped.
//!   2. Google Maps scrape — best effort, no browser; pulls the map centre out
//!      of the `/maps/place/` HTML. Fragile by nature.
//!   3. Google Geocoding API — only if `GOOGLE_MAPS_API_KEY` is set.

use crate::http::{HttpClient, HttpError};

pub use chintai_types::Geocoded;

#[derive(Debug, thiserror::Error)]
pub enum GeocodeError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error("response parse: {0}")]
    Parse(String),
}

#[async_trait::async_trait]
pub trait Geocoder: Send + Sync {
    /// `Ok(None)` = the backend worked but found nothing; `Err` = the backend
    /// itself failed (network, parse) so a fallback should be tried.
    async fn geocode(&self, query: &str) -> Result<Option<Geocoded>, GeocodeError>;
}

fn plausible_jp(lat: f64, lng: f64) -> bool {
    (20.0..46.5).contains(&lat) && (122.0..154.0).contains(&lng)
}

// --------------------------------------------------------------------------
// Nominatim
// --------------------------------------------------------------------------

pub struct NominatimGeocoder {
    http: HttpClient,
    base_url: String,
}

impl NominatimGeocoder {
    pub fn new(http: HttpClient) -> Self {
        // Pace is tuned in `crate::pacing::NOMINATIM`.
        http.set_host_interval("nominatim.openstreetmap.org", crate::pacing::NOMINATIM);
        Self {
            http,
            base_url: "https://nominatim.openstreetmap.org/search".to_owned(),
        }
    }
}

#[derive(serde::Deserialize)]
struct NominatimHit {
    lat: String,
    lon: String,
    display_name: String,
    #[serde(default)]
    name: Option<String>,
}

#[async_trait::async_trait]
impl Geocoder for NominatimGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<Geocoded>, GeocodeError> {
        let hits: Vec<NominatimHit> = self
            .http
            .get_json(
                &self.base_url,
                &[
                    ("q", query),
                    ("format", "jsonv2"),
                    ("addressdetails", "0"),
                    ("limit", "1"),
                    ("countrycodes", "jp"),
                    ("accept-language", "ja"),
                ],
            )
            .await?;

        let Some(hit) = hits.into_iter().next() else {
            return Ok(None);
        };
        let lat = hit
            .lat
            .parse()
            .map_err(|_| GeocodeError::Parse("lat".into()))?;
        let lng = hit
            .lon
            .parse()
            .map_err(|_| GeocodeError::Parse("lon".into()))?;
        let name = hit.name.filter(|s| !s.is_empty()).unwrap_or_else(|| {
            hit.display_name
                .split(',')
                .next()
                .unwrap_or("")
                .trim()
                .to_owned()
        });

        Ok(Some(Geocoded {
            name,
            address: hit.display_name,
            lat,
            lng,
            source: "nominatim".into(),
        }))
    }
}

// --------------------------------------------------------------------------
// Google Maps scrape (best effort)
// --------------------------------------------------------------------------

/// Pulls the map camera out of `/maps/place/` HTML. Without EU-style consent
/// cookies Google often serves a generic/near-empty map, so this can return a
/// confidently wrong coordinate — it is **off by default** in
/// [`default_geocoder`] and Nominatim is the real workhorse. Kept for the odd
/// case Nominatim has no data.
pub struct GoogleScrapeGeocoder {
    http: HttpClient,
}

impl GoogleScrapeGeocoder {
    pub fn new(http: HttpClient) -> Self {
        Self { http }
    }
}

#[async_trait::async_trait]
impl Geocoder for GoogleScrapeGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<Geocoded>, GeocodeError> {
        let url = format!(
            "https://www.google.com/maps/place/{}",
            urlencoding_min(query)
        );
        let html = self
            .http
            .get_text(&url, &[("hl", "ja"), ("gl", "jp")])
            .await?;

        // The `/maps/place/` document embeds the map camera as an early
        // `[[[<zoom>,<lng>,<lat>] ...` triple in APP_INITIALIZATION_STATE. Take
        // the first triple whose 2nd/3rd numbers land inside Japan.
        let Some((lat, lng)) = first_jp_lat_lng(&html) else {
            return Ok(None);
        };
        Ok(Some(Geocoded {
            name: query.to_owned(),
            address: query.to_owned(),
            lat,
            lng,
            source: "google_scrape".into(),
        }))
    }
}

/// Minimal percent-encoding for a path segment (spaces, slashes, non-ASCII).
fn urlencoding_min(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn first_jp_lat_lng(html: &str) -> Option<(f64, f64)> {
    // Scan every "[num,num,num]" run of three floats and accept the first that
    // looks like (?, lng, lat) for Japan.
    let bytes = html.as_bytes();
    let mut i = 0;
    while let Some(pos) = html[i..].find('[') {
        let start = i + pos + 1;
        // read up to 3 comma-separated floats
        let mut nums = [0f64; 3];
        let mut count = 0;
        let mut j = start;
        while count < 3 && j < bytes.len() {
            let (val, next) = match read_float(&html[j..]) {
                Some(v) => v,
                None => break,
            };
            nums[count] = val;
            count += 1;
            j += next;
            if bytes.get(j) == Some(&b',') {
                j += 1;
            } else {
                break;
            }
        }
        if count == 3 && plausible_jp(nums[2], nums[1]) {
            return Some((nums[2], nums[1]));
        }
        i = start;
    }
    None
}

/// Parse a leading float from `s`, returning (value, bytes consumed).
fn read_float(s: &str) -> Option<(f64, usize)> {
    let mut end = 0;
    let bytes = s.as_bytes();
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() || c == b'-' || c == b'.' || c == b'e' || c == b'E' || c == b'+' {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    s[..end].parse::<f64>().ok().map(|v| (v, end))
}

// --------------------------------------------------------------------------
// Google Geocoding API
// --------------------------------------------------------------------------

pub struct GoogleApiGeocoder {
    http: HttpClient,
    api_key: String,
}

impl GoogleApiGeocoder {
    pub fn new(http: HttpClient, api_key: String) -> Self {
        Self { http, api_key }
    }
}

#[derive(serde::Deserialize)]
struct GApiResp {
    status: String,
    #[serde(default)]
    results: Vec<GApiResult>,
}
#[derive(serde::Deserialize)]
struct GApiResult {
    formatted_address: String,
    geometry: GApiGeom,
}
#[derive(serde::Deserialize)]
struct GApiGeom {
    location: GApiLoc,
}
#[derive(serde::Deserialize)]
struct GApiLoc {
    lat: f64,
    lng: f64,
}

#[async_trait::async_trait]
impl Geocoder for GoogleApiGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<Geocoded>, GeocodeError> {
        let resp: GApiResp = self
            .http
            .get_json(
                "https://maps.googleapis.com/maps/api/geocode/json",
                &[
                    ("address", query),
                    ("language", "ja"),
                    ("region", "jp"),
                    ("key", &self.api_key),
                ],
            )
            .await?;

        if resp.status == "ZERO_RESULTS" {
            return Ok(None);
        }
        if resp.status != "OK" {
            return Err(GeocodeError::Parse(format!("status={}", resp.status)));
        }
        let Some(r) = resp.results.into_iter().next() else {
            return Ok(None);
        };
        Ok(Some(Geocoded {
            name: query.to_owned(),
            address: r.formatted_address,
            lat: r.geometry.location.lat,
            lng: r.geometry.location.lng,
            source: "google_api".into(),
        }))
    }
}

// --------------------------------------------------------------------------
// Fallback chain
// --------------------------------------------------------------------------

pub struct FallbackGeocoder {
    providers: Vec<Box<dyn Geocoder>>,
}

impl FallbackGeocoder {
    pub fn new(providers: Vec<Box<dyn Geocoder>>) -> Self {
        Self { providers }
    }
}

#[async_trait::async_trait]
impl Geocoder for FallbackGeocoder {
    async fn geocode(&self, query: &str) -> Result<Option<Geocoded>, GeocodeError> {
        let mut last_err = None;
        for p in &self.providers {
            match p.geocode(query).await {
                Ok(Some(hit)) if plausible_jp(hit.lat, hit.lng) => return Ok(Some(hit)),
                Ok(_) => {}
                Err(e) => {
                    log::warn!("geocoder failed, trying next: {e}");
                    last_err = Some(e);
                }
            }
        }
        match last_err {
            Some(e) => Err(e),
            None => Ok(None),
        }
    }
}

/// The standard chain: Nominatim, then Google scrape (best effort), then the
/// Google API if a key is available.
pub fn default_geocoder(
    http: HttpClient,
    google_api_key: Option<String>,
    enable_google_scrape: bool,
) -> FallbackGeocoder {
    let mut providers: Vec<Box<dyn Geocoder>> =
        vec![Box::new(NominatimGeocoder::new(http.clone()))];
    if enable_google_scrape {
        providers.push(Box::new(GoogleScrapeGeocoder::new(http.clone())));
    }
    if let Some(key) = google_api_key {
        providers.push(Box::new(GoogleApiGeocoder::new(http, key)));
    }
    FallbackGeocoder::new(providers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_first_japan_triple() {
        let s = r#"foo [1,2,3] bar [17.25,139.7005,35.6595] baz"#;
        let (lat, lng) = first_jp_lat_lng(s).unwrap();
        assert!((lat - 35.6595).abs() < 1e-9);
        assert!((lng - 139.7005).abs() < 1e-9);
    }

    #[test]
    fn ignores_non_japan_and_short_runs() {
        assert!(first_jp_lat_lng("[1,2] [999,999,999] [0,-73.9,40.7]").is_none());
    }

    #[test]
    fn google_state_shape() {
        let s = r#";window.APP_INITIALIZATION_STATE=[[[3,139.76608,35.68123],null,null]"#;
        let (lat, lng) = first_jp_lat_lng(s).unwrap();
        assert!((lat - 35.68123).abs() < 1e-6);
        assert!((lng - 139.76608).abs() < 1e-6);
    }

    #[test]
    fn read_float_consumes_expected() {
        assert_eq!(read_float("-35.68,x"), Some((-35.68, 6)));
        assert_eq!(read_float("abc"), None);
    }
}
