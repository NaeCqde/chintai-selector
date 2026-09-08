//! Small geo helpers. Distances are metres; "walk minutes" assume 80 m/min,
//! the same figure Japanese listing sites use (徒歩1分 = 80m).

pub const WALK_M_PER_MIN: f64 = 80.0;

pub fn haversine_m(a_lat: f64, a_lng: f64, b_lat: f64, b_lng: f64) -> f64 {
    const R: f64 = 6_371_000.0;
    let (p1, p2) = (a_lat.to_radians(), b_lat.to_radians());
    let dp = (b_lat - a_lat).to_radians();
    let dl = (b_lng - a_lng).to_radians();
    let x = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * R * x.sqrt().asin()
}

pub fn walk_minutes(distance_m: f64) -> u32 {
    (distance_m / WALK_M_PER_MIN).ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shibuya_to_shinjuku_is_about_3km() {
        let d = haversine_m(35.6580, 139.7016, 35.6896, 139.7006);
        assert!((d - 3_500.0).abs() < 400.0, "got {d}");
    }
}
