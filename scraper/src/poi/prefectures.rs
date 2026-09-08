//! Rough bounding boxes for the 47 prefectures, used to tile Overpass queries.
//! Boxes are generous (a little overlap is fine — upserts converge); islands far
//! offshore (Ogasawara, Daitō) are clipped to keep the query area sane.

/// (south, west, north, east)
pub type BBox = (f64, f64, f64, f64);

pub struct Prefecture {
    pub code: u32,
    pub name: &'static str,
    pub bbox: BBox,
}

pub fn by_code(code: u32) -> Option<&'static Prefecture> {
    PREFECTURES.iter().find(|p| p.code == code)
}

pub fn all() -> &'static [Prefecture] {
    PREFECTURES
}

/// Split a bbox into an `nx * ny` grid so a dense prefecture doesn't blow the
/// Overpass timeout.
pub fn tiles(b: BBox, nx: usize, ny: usize) -> Vec<BBox> {
    let (s, w, n, e) = b;
    let dh = (n - s) / ny as f64;
    let dw = (e - w) / nx as f64;
    let mut out = Vec::with_capacity(nx * ny);
    for iy in 0..ny {
        for ix in 0..nx {
            let ts = s + dh * iy as f64;
            let tw = w + dw * ix as f64;
            out.push((ts, tw, ts + dh, tw + dw));
        }
    }
    out
}

#[rustfmt::skip]
pub static PREFECTURES: &[Prefecture] = &[
    Prefecture { code: 1,  name: "北海道",   bbox: (41.35, 139.33, 45.55, 145.83) },
    Prefecture { code: 2,  name: "青森県",   bbox: (40.21, 139.49, 41.56, 141.68) },
    Prefecture { code: 3,  name: "岩手県",   bbox: (38.74, 140.65, 40.45, 142.07) },
    Prefecture { code: 4,  name: "宮城県",   bbox: (37.77, 140.27, 39.00, 141.68) },
    Prefecture { code: 5,  name: "秋田県",   bbox: (38.87, 139.42, 40.51, 141.02) },
    Prefecture { code: 6,  name: "山形県",   bbox: (37.73, 139.52, 39.22, 140.65) },
    Prefecture { code: 7,  name: "福島県",   bbox: (36.79, 139.16, 37.98, 141.05) },
    Prefecture { code: 8,  name: "茨城県",   bbox: (35.74, 139.68, 36.95, 140.86) },
    Prefecture { code: 9,  name: "栃木県",   bbox: (36.20, 139.32, 37.16, 140.30) },
    Prefecture { code: 10, name: "群馬県",   bbox: (35.99, 138.39, 37.06, 139.68) },
    Prefecture { code: 11, name: "埼玉県",   bbox: (35.75, 138.71, 36.29, 139.90) },
    Prefecture { code: 12, name: "千葉県",   bbox: (34.90, 139.74, 36.10, 140.87) },
    // Tokyo mainland + Tama (Izu/Ogasawara islands excluded); Kanagawa incl.
    // the Miura peninsula and the western 山北 tip.
    Prefecture { code: 13, name: "東京都",   bbox: (35.49, 138.90, 35.90, 139.93) },
    Prefecture { code: 14, name: "神奈川県", bbox: (35.10, 138.90, 35.68, 139.83) },
    Prefecture { code: 15, name: "新潟県",   bbox: (36.74, 137.62, 38.55, 139.90) },
    Prefecture { code: 16, name: "富山県",   bbox: (36.27, 136.77, 36.98, 137.75) },
    Prefecture { code: 17, name: "石川県",   bbox: (36.03, 136.23, 37.86, 137.36) },
    Prefecture { code: 18, name: "福井県",   bbox: (35.34, 135.45, 36.29, 136.50) },
    Prefecture { code: 19, name: "山梨県",   bbox: (35.17, 138.18, 35.97, 139.15) },
    Prefecture { code: 20, name: "長野県",   bbox: (35.20, 137.32, 37.03, 138.75) },
    Prefecture { code: 21, name: "岐阜県",   bbox: (35.13, 136.27, 36.47, 137.65) },
    Prefecture { code: 22, name: "静岡県",   bbox: (34.60, 137.47, 35.65, 139.17) },
    Prefecture { code: 23, name: "愛知県",   bbox: (34.57, 136.67, 35.43, 137.84) },
    Prefecture { code: 24, name: "三重県",   bbox: (33.72, 135.85, 35.26, 136.99) },
    Prefecture { code: 25, name: "滋賀県",   bbox: (34.79, 135.77, 35.71, 136.46) },
    Prefecture { code: 26, name: "京都府",   bbox: (34.70, 134.85, 35.78, 136.06) },
    Prefecture { code: 27, name: "大阪府",   bbox: (34.27, 135.09, 35.05, 135.74) },
    Prefecture { code: 28, name: "兵庫県",   bbox: (34.15, 134.25, 35.67, 135.47) },
    Prefecture { code: 29, name: "奈良県",   bbox: (33.86, 135.59, 34.78, 136.23) },
    Prefecture { code: 30, name: "和歌山県", bbox: (33.43, 134.99, 34.39, 136.02) },
    Prefecture { code: 31, name: "鳥取県",   bbox: (35.06, 133.13, 35.61, 134.51) },
    Prefecture { code: 32, name: "島根県",   bbox: (34.30, 131.67, 35.64, 133.39) },
    Prefecture { code: 33, name: "岡山県",   bbox: (34.30, 133.27, 35.35, 134.41) },
    Prefecture { code: 34, name: "広島県",   bbox: (34.02, 132.06, 35.11, 133.48) },
    Prefecture { code: 35, name: "山口県",   bbox: (33.71, 130.77, 34.79, 132.42) },
    Prefecture { code: 36, name: "徳島県",   bbox: (33.53, 133.65, 34.25, 134.81) },
    Prefecture { code: 37, name: "香川県",   bbox: (34.00, 133.45, 34.57, 134.45) },
    Prefecture { code: 38, name: "愛媛県",   bbox: (32.90, 132.00, 34.30, 133.68) },
    Prefecture { code: 39, name: "高知県",   bbox: (32.70, 132.48, 33.88, 134.31) },
    Prefecture { code: 40, name: "福岡県",   bbox: (33.02, 130.02, 34.02, 131.20) },
    Prefecture { code: 41, name: "佐賀県",   bbox: (32.95, 129.74, 33.61, 130.55) },
    Prefecture { code: 42, name: "長崎県",   bbox: (32.55, 128.58, 34.73, 130.40) },
    Prefecture { code: 43, name: "熊本県",   bbox: (32.10, 129.99, 33.21, 131.34) },
    Prefecture { code: 44, name: "大分県",   bbox: (32.71, 130.81, 33.74, 132.08) },
    Prefecture { code: 45, name: "宮崎県",   bbox: (31.36, 130.71, 32.84, 131.90) },
    Prefecture { code: 46, name: "鹿児島県", bbox: (30.36, 129.55, 32.21, 131.20) },
    Prefecture { code: 47, name: "沖縄県",   bbox: (24.04, 122.93, 27.00, 128.40) },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_47_present_and_unique() {
        assert_eq!(PREFECTURES.len(), 47);
        let mut codes: Vec<_> = PREFECTURES.iter().map(|p| p.code).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 47);
        assert_eq!(by_code(13).unwrap().name, "東京都");
    }

    #[test]
    fn tiles_cover_the_box() {
        let b = (0.0, 0.0, 10.0, 10.0);
        let t = tiles(b, 2, 2);
        assert_eq!(t.len(), 4);
        assert_eq!(t[0], (0.0, 0.0, 5.0, 5.0));
        assert_eq!(t[3], (5.0, 5.0, 10.0, 10.0));
    }
}
