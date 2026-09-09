//! Walk-segment elevation via the GSI (国土地理院) elevation API — free, no key,
//! Japan coverage. One point per request, so we sample the path down to a
//! handful of points and pace requests with the shared rate limiter.

use super::polyline;
use super::types::LatLng;
use crate::HttpClient;

const GSI_URL: &str = "https://cyberjapandata2.gsi.go.jp/general/dem/scripts/getelevation.php";
const MAX_SAMPLES: usize = 16;

#[derive(Clone)]
pub struct GsiElevation {
    http: HttpClient,
}

#[derive(serde::Deserialize)]
struct GsiResp {
    /// A number, or the string "-----" where there is no DEM data.
    elevation: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ElevationDelta {
    pub ascent_m: f64,
    pub descent_m: f64,
}

impl GsiElevation {
    pub fn new(http: HttpClient) -> Self {
        // Pace is tuned in `crate::pacing::GSI_ELEVATION`.
        http.set_host_interval("cyberjapandata2.gsi.go.jp", crate::pacing::GSI_ELEVATION);
        Self { http }
    }

    /// Total metres climbed / descended along an encoded polyline. `None` if we
    /// couldn't get enough samples to say anything.
    pub async fn delta_for_polyline(&self, encoded: &str) -> Option<ElevationDelta> {
        let path = polyline::decode(encoded);
        self.delta_for_path(&path).await
    }

    pub async fn delta_for_path(&self, path: &[LatLng]) -> Option<ElevationDelta> {
        let pts = polyline::sample(path, MAX_SAMPLES);
        if pts.len() < 2 {
            return None;
        }

        let mut elevations = Vec::with_capacity(pts.len());
        for p in &pts {
            match self.elevation_at(*p).await {
                Some(e) => elevations.push(e),
                None => continue,
            }
        }
        if elevations.len() < 2 {
            return None;
        }

        let mut d = ElevationDelta::default();
        for w in elevations.windows(2) {
            let diff = w[1] - w[0];
            if diff > 0.0 {
                d.ascent_m += diff;
            } else {
                d.descent_m += -diff;
            }
        }
        Some(d)
    }

    async fn elevation_at(&self, p: LatLng) -> Option<f64> {
        let (lat, lng) = (p.lat.to_string(), p.lng.to_string());
        let resp: GsiResp = self
            .http
            .get_json(
                GSI_URL,
                &[("lon", &lng), ("lat", &lat), ("outtype", "JSON")],
            )
            .await
            .ok()?;
        match resp.elevation {
            serde_json::Value::Number(n) => n.as_f64(),
            serde_json::Value::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}
