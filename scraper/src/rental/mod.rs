//! Rental-listing collection: SUUMO and LIFULL HOME'S list-page adapters, plus
//! the persistence that resolves scraped rows onto canonical `property` /
//! `unit` records (de-duplicating the same room seen on both sites) and keeps
//! the raw per-site row in `listing`.

pub mod homes;
pub mod normalize;
pub mod suumo;

use chintai_db::entities::enums::ListingSite;
use chintai_db::{
    record_listing, upsert_property, upsert_unit, DatabaseConnection, NewListing, NewProperty,
    NewUnit,
};

use crate::http::HttpError;
use crate::HttpClient;

pub use homes::Homes;
pub use suumo::Suumo;

#[derive(Debug, thiserror::Error)]
pub enum RentalError {
    #[error(transparent)]
    Http(#[from] HttpError),
    #[error(transparent)]
    Db(#[from] chintai_db::DbErr),
    #[error("parse: {0}")]
    Parse(String),
}

/// A building as scraped from one list page.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ScrapedBuilding {
    pub name: String,
    pub address: String,
    pub stations: Vec<StationAccess>,
    pub built_year: Option<i32>,
    pub floors: Option<i32>,
    pub structure: Option<String>,
    pub source_building_id: String,
    pub units: Vec<ScrapedUnit>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct StationAccess {
    pub line: String,
    pub station: String,
    pub walk_min: Option<i32>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ScrapedUnit {
    pub floor: Option<String>,
    pub layout: Option<String>,
    pub area_m2: Option<f64>,
    pub rent_yen: Option<i32>,
    pub admin_fee_yen: Option<i32>,
    pub deposit_yen: Option<i32>,
    pub key_money_yen: Option<i32>,
    pub source_unit_id: String,
    pub url: String,
}

impl ScrapedUnit {
    /// Stable within a property across sites: floor + layout + area(0.5 m²) +
    /// rent(¥1000). The same physical room lands on the same key.
    pub fn dedup_key(&self) -> String {
        let floor = self.floor.as_deref().unwrap_or("?").trim();
        let layout = self.layout.as_deref().unwrap_or("?").trim();
        let area = self.area_m2.map(|a| (a * 2.0).round() as i64).unwrap_or(0);
        let rent = self.rent_yen.map(|r| r / 1000).unwrap_or(0);
        format!("{floor}|{layout}|{area}|{rent}")
    }
}

#[async_trait::async_trait]
pub trait ListingSource: Send + Sync {
    fn site(&self) -> ListingSite;
    /// `area` is a site-specific path fragment (see each adapter). Returns the
    /// buildings on `page` (1-based); an empty vec means "past the last page".
    async fn list_page(&self, area: &str, page: u32) -> Result<Vec<ScrapedBuilding>, RentalError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct RentalCounts {
    pub pages: u32,
    pub buildings: u64,
    pub units: u64,
    pub listings: u64,
}

/// Walk an area page by page and persist. Stops at an empty page or `max_pages`.
pub async fn ingest_area(
    db: &DatabaseConnection,
    source: &dyn ListingSource,
    area: &str,
    max_pages: u32,
) -> Result<RentalCounts, RentalError> {
    let site = source.site();
    let mut counts = RentalCounts::default();

    for page in 1..=max_pages {
        let buildings = source.list_page(area, page).await?;
        if buildings.is_empty() {
            break;
        }
        counts.pages = page;
        log::info!("{site:?} {area} p{page}: {} buildings", buildings.len());

        for b in &buildings {
            let prop = upsert_property(
                db,
                NewProperty {
                    // Canonical key: normalised name + address so SUUMO and
                    // HOME'S rows for one building collapse to one property.
                    name: normalize::norm_building_name(&b.name),
                    address: normalize::norm_address(&b.address),
                    lat: None,
                    lng: None,
                    building_type: b.structure.clone(),
                    built_year: b.built_year,
                    structure: b.structure.clone(),
                },
            )
            .await?;
            counts.buildings += 1;

            // Approximate coordinates from the nearest scraped station (the
            // list page has no lat/lng, and Nominatim can't pin a coarse ward
            // address). Good enough for the ranking engine's routing; a later
            // `ingest geocode` pass refines what's left.
            if prop.lat.is_none() {
                for st in &b.stations {
                    if let Ok(Some(station)) = chintai_db::station_by_name(db, &st.station).await {
                        let _ =
                            chintai_db::set_property_coords(db, prop.id, station.lat, station.lng)
                                .await;
                        break;
                    }
                }
            }

            for u in &b.units {
                let unit = upsert_unit(
                    db,
                    NewUnit {
                        property_id: prop.id,
                        dedup_key: u.dedup_key(),
                        floor: u.floor.clone(),
                        layout: u.layout.clone(),
                        area_m2: u.area_m2,
                        rent_yen: u.rent_yen,
                        admin_fee_yen: u.admin_fee_yen,
                        deposit_yen: u.deposit_yen,
                        key_money_yen: u.key_money_yen,
                        available_from: None,
                    },
                )
                .await?;
                counts.units += 1;

                record_listing(
                    db,
                    NewListing {
                        unit_id: Some(unit.id),
                        site,
                        site_listing_id: u.source_unit_id.clone(),
                        url: u.url.clone(),
                        raw_json: serde_json::json!({
                            "building": b.name,
                            "address": b.address,
                            "stations": b.stations,
                            "unit": u,
                        })
                        .to_string(),
                    },
                )
                .await?;
                counts.listings += 1;
            }
        }
    }

    Ok(counts)
}

/// Current calendar year (for "築N年" → build year). Cheap approximation from
/// the Unix clock; a day or two of drift near New Year does not matter here.
pub(crate) fn current_year() -> i32 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    1970 + (secs / 31_556_952) as i32
}

/// Shared helper for the adapters: fetch a Japanese (Shift_JIS) list page.
pub(crate) async fn fetch_html(
    http: &HttpClient,
    url: &str,
    query: &[(&str, &str)],
) -> Result<String, RentalError> {
    Ok(http.get_html_jp(url, query).await?)
}
