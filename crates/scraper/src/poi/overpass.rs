//! Bulk POI ingest from the OpenStreetMap Overpass API: convenience stores,
//! supermarkets, parcel lockers, post offices and rail stations for a bounding
//! box (or a whole prefecture, tiled).

use chintai_db::entities::enums::{BrandKind, StoreSource};
use chintai_db::{
    upsert_brand, upsert_delivery, upsert_station, upsert_store, DatabaseConnection, NewDelivery,
    NewStation, NewStore,
};

use super::brand_master::{carrier_of, classify};
use super::prefectures::{self, BBox};
use crate::http::HttpError;
use crate::HttpClient;

/// The public Overpass instances, tried in order — the main one frequently
/// answers 429/504 under load, the mirrors pick up the slack.
const ENDPOINTS: &[&str] = &[
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.openstreetmap.fr/api/interpreter",
    "https://overpass.private.coffee/api/interpreter",
];

#[derive(Debug, thiserror::Error)]
pub enum OverpassError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Db(#[from] chintai_db::DbErr),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IngestCounts {
    pub convenience: u64,
    pub supermarket: u64,
    pub parcel: u64,
    pub post_office: u64,
    pub station: u64,
    pub skipped: u64,
}

impl IngestCounts {
    fn add(&mut self, o: IngestCounts) {
        self.convenience += o.convenience;
        self.supermarket += o.supermarket;
        self.parcel += o.parcel;
        self.post_office += o.post_office;
        self.station += o.station;
        self.skipped += o.skipped;
    }

    pub fn total_pois(&self) -> u64 {
        self.convenience + self.supermarket + self.parcel + self.post_office + self.station
    }
}

pub struct Overpass {
    http: HttpClient,
}

impl Default for Overpass {
    fn default() -> Self {
        Self::new()
    }
}

impl Overpass {
    /// Builds its own client with a 210 s read timeout — a dense prefecture
    /// tile can legitimately take a couple of minutes to compute server-side
    /// (`[timeout:180]` in the QL).
    pub fn new() -> Self {
        let http = HttpClient::slow(
            std::time::Duration::from_secs(4),
            std::time::Duration::from_secs(210),
        );
        for host in [
            "overpass-api.de",
            "overpass.kumi.systems",
            "overpass.openstreetmap.fr",
            "overpass.private.coffee",
        ] {
            http.set_host_interval(host, std::time::Duration::from_secs(4));
        }
        Self { http }
    }

    fn ql(bbox: BBox) -> String {
        let (s, w, n, e) = bbox;
        format!(
            "[out:json][timeout:180];\
             (\
               nwr[\"shop\"=\"convenience\"]({s},{w},{n},{e});\
               nwr[\"shop\"=\"supermarket\"]({s},{w},{n},{e});\
               nwr[\"amenity\"=\"parcel_locker\"]({s},{w},{n},{e});\
               nwr[\"amenity\"=\"post_office\"]({s},{w},{n},{e});\
               nwr[\"railway\"=\"station\"]({s},{w},{n},{e});\
             );\
             out center tags;"
        )
    }

    pub async fn fetch(&self, bbox: BBox) -> Result<Vec<Element>, OverpassError> {
        let ql = Self::ql(bbox);
        let mut last: Option<HttpError> = None;
        for (i, endpoint) in ENDPOINTS.iter().enumerate() {
            match self
                .http
                .get_json::<OverpassResponse>(endpoint, &[("data", &ql)])
                .await
            {
                Ok(resp) => return Ok(resp.elements),
                Err(e) => {
                    log::warn!(
                        "overpass {endpoint} failed ({e}); {} mirror(s) left",
                        ENDPOINTS.len() - i - 1
                    );
                    last = Some(e);
                }
            }
        }
        Err(OverpassError::Http(last.unwrap_or(HttpError::BadUrl(
            "no overpass endpoint".into(),
        ))))
    }

    /// Fetch one bbox and upsert everything it returns.
    pub async fn ingest_bbox(
        &self,
        db: &DatabaseConnection,
        bbox: BBox,
    ) -> Result<IngestCounts, OverpassError> {
        let elements = self.fetch(bbox).await?;
        let mut counts = IngestCounts::default();

        for el in &elements {
            let Some((lat, lng)) = el.coord() else {
                counts.skipped += 1;
                continue;
            };
            let ext = format!("{}/{}", el.el_type, el.id);
            let tags = &el.tags;

            match el.category() {
                Some(Category::Convenience) | Some(Category::Supermarket) => {
                    let kind = if matches!(el.category(), Some(Category::Convenience)) {
                        BrandKind::ConvenienceStore
                    } else {
                        BrandKind::Supermarket
                    };
                    let Some(guess) = classify(kind, tags) else {
                        counts.skipped += 1;
                        continue;
                    };
                    let brand = upsert_brand(db, &guess.name, guess.kind, guess.price_tier).await?;
                    upsert_store(
                        db,
                        NewStore {
                            brand_id: brand.id,
                            name: el.name().unwrap_or(&guess.name).to_owned(),
                            lat: Some(lat),
                            lng: Some(lng),
                            address: el.address(),
                            source: Some(StoreSource::Osm),
                            external_id: Some(ext),
                        },
                    )
                    .await?;
                    match kind {
                        BrandKind::ConvenienceStore => counts.convenience += 1,
                        BrandKind::Supermarket => counts.supermarket += 1,
                    }
                }
                Some(Category::ParcelLocker) | Some(Category::PostOffice) => {
                    let is_post = matches!(el.category(), Some(Category::PostOffice));
                    let carrier = if is_post {
                        "japanpost".to_owned()
                    } else {
                        carrier_of(tags)
                    };
                    upsert_delivery(
                        db,
                        NewDelivery {
                            store_id: None,
                            name: el.name().map(str::to_owned),
                            lat: Some(lat),
                            lng: Some(lng),
                            address: el.address(),
                            carrier,
                            external_id: Some(ext),
                            can_ship: true,
                            can_receive: true,
                        },
                    )
                    .await?;
                    if is_post {
                        counts.post_office += 1;
                    } else {
                        counts.parcel += 1;
                    }
                }
                Some(Category::Station) => {
                    upsert_station(
                        db,
                        NewStation {
                            name: el.name().unwrap_or("駅").to_owned(),
                            lat,
                            lng,
                            address: el.address(),
                            osm_id: Some(ext),
                        },
                    )
                    .await?;
                    counts.station += 1;
                }
                None => counts.skipped += 1,
            }
        }

        Ok(counts)
    }

    /// Ingest a whole prefecture by code (1–47), tiling the bbox to stay under
    /// the Overpass timeout.
    pub async fn ingest_prefecture(
        &self,
        db: &DatabaseConnection,
        code: u32,
        grid: (usize, usize),
    ) -> Result<IngestCounts, OverpassError> {
        let Some(pref) = prefectures::by_code(code) else {
            return Ok(IngestCounts::default());
        };
        let mut total = IngestCounts::default();
        let tiles = prefectures::tiles(pref.bbox, grid.0, grid.1);
        for (i, t) in tiles.iter().enumerate() {
            log::info!(
                "overpass {} tile {}/{} {:?}",
                pref.name,
                i + 1,
                tiles.len(),
                t
            );
            match self.ingest_bbox(db, *t).await {
                Ok(c) => {
                    log::info!("  +{} pois ({} skipped)", c.total_pois(), c.skipped);
                    total.add(c);
                }
                Err(e) => log::warn!("  tile failed: {e}"),
            }
        }
        Ok(total)
    }
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum Category {
    Convenience,
    Supermarket,
    ParcelLocker,
    PostOffice,
    Station,
}

#[derive(serde::Deserialize)]
pub struct OverpassResponse {
    #[serde(default)]
    pub elements: Vec<Element>,
}

#[derive(serde::Deserialize)]
pub struct Element {
    #[serde(rename = "type")]
    pub el_type: String,
    pub id: i64,
    #[serde(default)]
    lat: Option<f64>,
    #[serde(default)]
    lon: Option<f64>,
    #[serde(default)]
    center: Option<Center>,
    #[serde(default)]
    pub tags: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct Center {
    lat: f64,
    lon: f64,
}

impl Element {
    fn coord(&self) -> Option<(f64, f64)> {
        match (self.lat, self.lon) {
            (Some(a), Some(o)) => Some((a, o)),
            _ => self.center.as_ref().map(|c| (c.lat, c.lon)),
        }
    }

    fn tag(&self, k: &str) -> Option<&str> {
        self.tags.get(k).and_then(|v| v.as_str())
    }

    fn name(&self) -> Option<&str> {
        self.tag("name").or_else(|| self.tag("name:ja"))
    }

    fn address(&self) -> Option<String> {
        let parts: Vec<&str> = [
            "addr:province",
            "addr:city",
            "addr:suburb",
            "addr:neighbourhood",
            "addr:block_number",
            "addr:housenumber",
        ]
        .iter()
        .filter_map(|k| self.tag(k))
        .collect();
        (!parts.is_empty()).then(|| parts.join(""))
    }

    fn category(&self) -> Option<Category> {
        match self.tag("shop") {
            Some("convenience") => return Some(Category::Convenience),
            Some("supermarket") => return Some(Category::Supermarket),
            _ => {}
        }
        match self.tag("amenity") {
            Some("parcel_locker") => return Some(Category::ParcelLocker),
            Some("post_office") => return Some(Category::PostOffice),
            _ => {}
        }
        if self.tag("railway") == Some("station") {
            return Some(Category::Station);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn el(json: serde_json::Value) -> Element {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn parses_node_and_way_coords() {
        let node = el(serde_json::json!({
            "type": "node", "id": 1, "lat": 35.6, "lon": 139.7,
            "tags": {"shop": "convenience", "brand": "ローソン"}
        }));
        assert_eq!(node.coord(), Some((35.6, 139.7)));
        assert!(matches!(node.category(), Some(Category::Convenience)));

        let way = el(serde_json::json!({
            "type": "way", "id": 2, "center": {"lat": 35.1, "lon": 139.1},
            "tags": {"shop": "supermarket", "name": "オーケー 中野店"}
        }));
        assert_eq!(way.coord(), Some((35.1, 139.1)));
        assert!(matches!(way.category(), Some(Category::Supermarket)));
    }

    #[test]
    fn builds_address_from_addr_tags() {
        let e = el(serde_json::json!({
            "type": "node", "id": 3, "lat": 1.0, "lon": 2.0,
            "tags": {"railway": "station", "name": "渋谷駅",
                     "addr:city": "渋谷区", "addr:suburb": "道玄坂"}
        }));
        assert_eq!(e.address().as_deref(), Some("渋谷区道玄坂"));
        assert!(matches!(e.category(), Some(Category::Station)));
    }

    #[test]
    fn ql_has_all_five_selectors() {
        let q = Overpass::ql((35.0, 139.0, 36.0, 140.0));
        for s in [
            "shop\"=\"convenience",
            "shop\"=\"supermarket",
            "amenity\"=\"parcel_locker",
            "amenity\"=\"post_office",
            "railway\"=\"station",
        ] {
            assert!(q.contains(s), "missing {s}");
        }
    }
}
