//! AtHome (at-home.co.jp) list-page adapter.
//!
//! `area` is the path fragment after `/chintai/`, e.g. `tokyo/setagaya-city`
//! (same convention as [`super::homes::Homes`]). URL is
//! `https://www.athome.co.jp/chintai/<area>/list/` for page 1 and
//! `.../list/page<N>/` for page N>1. No browser UA needed — a plain
//! `HttpClient::new()` works. Structure: `.property-card` per building,
//! `.room-info-section` per unit.

use scraper::{ElementRef, Html, Selector};

use super::normalize::*;
use super::{ListingSource, RentalError, ScrapedBuilding, ScrapedUnit, StationAccess};
use crate::HttpClient;
use chintai_db::entities::enums::ListingSite;

pub struct Athome {
    http: HttpClient,
    this_year: i32,
}

impl Athome {
    pub fn new(http: HttpClient) -> Self {
        Self {
            http,
            this_year: super::current_year(),
        }
    }
}

fn text(el: ElementRef) -> String {
    el.text()
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn sel(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

#[async_trait::async_trait]
impl ListingSource for Athome {
    fn site(&self) -> ListingSite {
        ListingSite::Athome
    }

    async fn list_page(&self, area: &str, page: u32) -> Result<Vec<ScrapedBuilding>, RentalError> {
        let url = if page <= 1 {
            format!("https://www.athome.co.jp/chintai/{area}/list/")
        } else {
            format!("https://www.athome.co.jp/chintai/{area}/list/page{page}/")
        };
        let html = super::fetch_html(&self.http, &url, &[]).await?;
        Ok(parse_list(&html, self.this_year))
    }
}

pub(crate) fn parse_list(html: &str, this_year: i32) -> Vec<ScrapedBuilding> {
    let doc = Html::parse_document(html);
    let item = sel(".property-card");
    let title_sel = sel(".property-title");
    let loc_sel = sel(".info-item--location");
    let station_sel = sel(".info-item--station p");
    let type_sel = sel(".info-item--type");
    let room_sel = sel(".room-info-section");

    let mut out = Vec::new();
    for b in doc.select(&item) {
        let name = b.select(&title_sel).next().map(text).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let address = b.select(&loc_sel).next().map(text).unwrap_or_default();
        let stations = b
            .select(&station_sel)
            .filter_map(|s| parse_station(&text(s)))
            .collect::<Vec<_>>();

        // ".info-item--type" is two *separate* text nodes joined by a <br> —
        // "賃貸マンション 3階建" then "2022年3月(築4年7ヶ月)" — so pull text
        // nodes individually rather than joining+re-splitting on whitespace
        // (that glues "3階建" and "2022年..." into one token with no digit
        // boundary between them).
        let (mut built_year, mut floors, mut structure) = (None, None, None);
        if let Some(t) = b.select(&type_sel).next() {
            let lines: Vec<String> = t
                .text()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect();
            if let Some(first) = lines.first() {
                structure = first
                    .split_whitespace()
                    .next()
                    .map(|s| s.trim_start_matches("賃貸").to_owned());
                floors = first
                    .split_whitespace()
                    .find(|p| p.ends_with("階建"))
                    .and_then(digits_i32);
            }
            // The date shown is the actual completion year (e.g. "2022年3月"),
            // not an age — unlike SUUMO/HOME'S "築N年" text, so no this_year
            // arithmetic. "新築" (no date line at all) falls back to this_year.
            built_year = lines
                .get(1)
                .and_then(|l| digits_i32(l.split('年').next().unwrap_or(l)))
                .or_else(|| {
                    lines
                        .first()
                        .is_some_and(|l| l.contains("新築"))
                        .then_some(this_year)
                });
        }

        let units = b
            .select(&room_sel)
            .filter_map(parse_unit)
            .collect::<Vec<_>>();
        if units.is_empty() {
            continue;
        }

        out.push(ScrapedBuilding {
            source_building_id: stable_id(&name, &address),
            name,
            address,
            stations,
            built_year,
            floors,
            structure,
            units,
        });
    }
    out
}

fn parse_unit(row: ElementRef) -> Option<ScrapedUnit> {
    let link_sel = sel(".room-info-section__room-detail a");
    let room_no_sel = sel(".room-number .room-info-item__text");
    let rent_sel = sel(".price .rent-value");
    let admin_sel = sel(".price .room-info-item__text:not(.rent)");
    let fee_sel = sel(".fees .room-info-item__text");
    let layout_size_sel = sel(".layout-size .room-info-item__text");

    let href = row.select(&link_sel).next()?.value().attr("href")?;
    let id = href
        .trim_start_matches('/')
        .split('/')
        .nth(1)
        .filter(|s| !s.is_empty())?
        .to_owned();

    let room_no = row
        .select(&room_no_sel)
        .next()
        .map(text)
        .unwrap_or_default();
    let floor = floor_from_room_number(&room_no);

    let rent_yen = row
        .select(&rent_sel)
        .next()
        .map(text)
        .and_then(|n| parse_yen(&format!("{n}万円")));
    let admin_fee_yen = row
        .select(&admin_sel)
        .next()
        .and_then(|e| parse_yen(&text(e)));

    let fees: Vec<String> = row.select(&fee_sel).map(text).collect();
    let deposit_yen = fees.first().and_then(|s| parse_yen(s));
    let key_money_yen = fees.get(1).and_then(|s| parse_yen(s));

    let sizes: Vec<String> = row.select(&layout_size_sel).map(text).collect();
    let layout = sizes.first().cloned().filter(|s| !s.is_empty());
    let area_m2 = sizes.get(1).and_then(|s| parse_area(s));

    Some(ScrapedUnit {
        floor,
        layout,
        area_m2,
        rent_yen,
        admin_fee_yen,
        deposit_yen,
        key_money_yen,
        url: format!("https://www.athome.co.jp/chintai/{id}/"),
        source_unit_id: id,
    })
}

/// AtHome shows a room number like "２０４" (fullwidth) instead of a floor —
/// by Japanese apartment-numbering convention the leading digit(s) are the
/// floor: 3-digit -> 1 digit, 4-digit -> 2 digits.
fn floor_from_room_number(room_no: &str) -> Option<String> {
    let digits: String = to_halfwidth(room_no)
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    let take = match digits.len() {
        4 => 2,
        3 => 1,
        _ => return None,
    };
    Some(format!("{}階", &digits[..take]))
}

/// "京王線 「千歳烏山」駅 徒歩13分" -> line "京王線", station "千歳烏山駅"
/// (the 「」 corner brackets around the bare station name are AtHome's own
/// convention, not SUUMO/HOME'S "line/station 歩N分" shape, so this doesn't
/// reuse `suumo::parse_station`).
fn parse_station(s: &str) -> Option<StationAccess> {
    let t = to_halfwidth(s);
    let walk_min = parse_walk_min(&t);
    let open = t.find('「')?;
    let close = t.find('」')?;
    let line = t[..open].trim().to_string();
    let station = format!("{}駅", t[open + '「'.len_utf8()..close].trim());
    Some(StationAccess {
        line,
        station,
        walk_min,
    })
}

fn digits_i32(s: &str) -> Option<i32> {
    let d: String = to_halfwidth(s)
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    d.parse().ok()
}

fn stable_id(name: &str, address: &str) -> String {
    let mut h: u64 = 1469598103934665603;
    for byte in norm_building_name(name)
        .bytes()
        .chain(norm_address(address).bytes())
    {
        h ^= byte as u64;
        h = h.wrapping_mul(1099511628211);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_setagaya_fixture() {
        let html = include_str!("../../tests/fixtures/athome_setagaya.html");
        let buildings = parse_list(html, 2026);
        assert!(buildings.len() >= 3, "got {}", buildings.len());

        let b = buildings.iter().find(|b| !b.units.is_empty()).unwrap();
        assert!(!b.name.is_empty());
        assert!(b.address.contains("世田谷"));
        assert!(!b.stations.is_empty());
        assert_eq!(b.stations[0].line, "京王線");
        assert_eq!(
            b.stations[0].station, "千歳烏山駅",
            "brackets must be stripped"
        );
        assert_eq!(b.stations[0].walk_min, Some(13));
        assert_eq!(b.floors, Some(3));
        assert_eq!(
            b.built_year,
            Some(2026),
            "must not glue onto the floor digit"
        );
        assert_eq!(b.structure.as_deref(), Some("マンション"));

        let u = &b.units[0];
        assert!(u.rent_yen.unwrap_or(0) > 10_000, "rent {:?}", u.rent_yen);
        assert!(u.area_m2.unwrap_or(0.0) > 5.0, "area {:?}", u.area_m2);
        assert!(!u.source_unit_id.is_empty());
        assert!(u.url.starts_with("https://www.athome.co.jp/chintai/"));
        assert_eq!(u.floor.as_deref(), Some("2階"));

        // A building further down the fixture whose age is given as "築N年"
        // rather than a bare new-build date.
        let aged = buildings
            .iter()
            .find(|b| b.name.contains("等々力"))
            .unwrap();
        assert_eq!(aged.built_year, Some(2022));
    }

    #[test]
    fn floor_from_room_number_works() {
        assert_eq!(floor_from_room_number("２０４"), Some("2階".into()));
        assert_eq!(floor_from_room_number("１２０４"), Some("12階".into()));
        assert_eq!(floor_from_room_number(""), None);
    }

    #[test]
    fn station_brackets_and_romaji_line() {
        let s = parse_station("東急田園都市線 「用賀」駅 徒歩7分").unwrap();
        assert_eq!(s.line, "東急田園都市線");
        assert_eq!(s.station, "用賀駅");
        assert_eq!(s.walk_min, Some(7));
    }
}
