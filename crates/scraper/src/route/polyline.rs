//! Google "encoded polyline algorithm" decoder (precision 5). Used to walk the
//! path returned by the Routes API so the elevation sampler has coordinates.

use super::types::LatLng;

pub fn decode(encoded: &str) -> Vec<LatLng> {
    let bytes = encoded.as_bytes();
    let mut idx = 0;
    let mut lat = 0i64;
    let mut lng = 0i64;
    let mut out = Vec::new();

    while idx < bytes.len() {
        let Some(dlat) = next_value(bytes, &mut idx) else {
            break;
        };
        let Some(dlng) = next_value(bytes, &mut idx) else {
            break;
        };
        lat += dlat;
        lng += dlng;
        out.push(LatLng::new(lat as f64 * 1e-5, lng as f64 * 1e-5));
    }
    out
}

fn next_value(bytes: &[u8], idx: &mut usize) -> Option<i64> {
    let mut shift = 0u32;
    let mut result = 0i64;
    loop {
        if *idx >= bytes.len() {
            return None;
        }
        let b = (bytes[*idx] as i64) - 63;
        *idx += 1;
        result |= (b & 0x1f) << shift;
        shift += 5;
        if b < 0x20 {
            break;
        }
    }
    let delta = if result & 1 != 0 {
        !(result >> 1)
    } else {
        result >> 1
    };
    Some(delta)
}

/// Down-sample a path to at most `max` points, keeping the endpoints.
pub fn sample(path: &[LatLng], max: usize) -> Vec<LatLng> {
    if path.len() <= max || max < 2 {
        return path.to_vec();
    }
    let step = (path.len() - 1) as f64 / (max - 1) as f64;
    (0..max)
        .map(|i| path[(i as f64 * step).round() as usize])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_the_wikipedia_example() {
        // From Google's polyline docs: "_p~iF~ps|U_ulLnnqC_mqNvxq`@"
        let pts = decode("_p~iF~ps|U_ulLnnqC_mqNvxq`@");
        assert_eq!(pts.len(), 3);
        assert!((pts[0].lat - 38.5).abs() < 1e-6);
        assert!((pts[0].lng + 120.2).abs() < 1e-6);
        assert!((pts[1].lat - 40.7).abs() < 1e-6);
        assert!((pts[1].lng + 120.95).abs() < 1e-6);
        assert!((pts[2].lat - 43.252).abs() < 1e-6);
        assert!((pts[2].lng + 126.453).abs() < 1e-6);
    }

    #[test]
    fn sample_keeps_endpoints() {
        let path: Vec<_> = (0..100).map(|i| LatLng::new(i as f64, 0.0)).collect();
        let s = sample(&path, 5);
        assert_eq!(s.len(), 5);
        assert_eq!(s[0].lat, 0.0);
        assert_eq!(s[4].lat, 99.0);
    }
}
