//! SUUMO list-page adapter.
//!
//! `area` is the path fragment after `/chintai/`, e.g. `tokyo/sc_setagaya`.
//! Pages are `?page=N` (1-based). Structure: `.cassetteitem` per building, a
//! `table.cassetteitem_other` inside it with one `tr.js-cassette_link` per unit.

use scraper::{ElementRef, Html, Selector};

use super::normalize::*;
use super::{ListingSource, RentalError, ScrapedBuilding, ScrapedUnit, StationAccess};
use crate::HttpClient;
use chintai_db::entities::enums::ListingSite;

pub struct Suumo {
    http: HttpClient,
    this_year: i32,
}

impl Suumo {
    pub fn new(http: HttpClient) -> Self {
        // Pace is tuned in `crate::pacing::SUUMO`.
        http.set_host_interval("suumo.jp", crate::pacing::SUUMO);
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
impl ListingSource for Suumo {
    fn site(&self) -> ListingSite {
        ListingSite::Suumo
    }

    async fn list_page(&self, area: &str, page: u32) -> Result<Vec<ScrapedBuilding>, RentalError> {
        let url = format!("https://suumo.jp/chintai/{area}/");
        let page_s = page.to_string();
        let html = super::fetch_html(&self.http, &url, &[("page", &page_s)]).await?;
        Ok(parse_list(&html, self.this_year))
    }
}

pub(crate) fn parse_list(html: &str, this_year: i32) -> Vec<ScrapedBuilding> {
    let doc = Html::parse_document(html);
    let item = sel(".cassetteitem");
    let title = sel(".cassetteitem_content-title");
    let col1 = sel(".cassetteitem_detail-col1");
    let col2_text = sel(".cassetteitem_detail-col2 .cassetteitem_detail-text");
    let col3_div = sel(".cassetteitem_detail-col3 div");
    let unit_row = sel("table.cassetteitem_other tbody tr.js-cassette_link");

    let mut out = Vec::new();
    for b in doc.select(&item) {
        let name = b.select(&title).next().map(text).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let address = b.select(&col1).next().map(text).unwrap_or_default();

        let stations = b
            .select(&col2_text)
            .filter_map(|t| parse_station(&text(t)))
            .collect::<Vec<_>>();

        let mut built_year = None;
        let mut floors = None;
        for d in b.select(&col3_div) {
            let t = text(d);
            if t.contains('築') || t.contains("新築") {
                built_year = parse_built_year(&t, this_year);
            } else if t.contains('階') {
                floors = digits_i32(&t);
            }
        }

        let units = b
            .select(&unit_row)
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
            structure: None,
            units,
        });
    }
    out
}

fn parse_unit(row: ElementRef) -> Option<ScrapedUnit> {
    let id_sel = sel("input.js-single_checkbox, input.js-clipkey");
    let rent_sel = sel(".cassetteitem_price--rent");
    let admin_sel = sel(".cassetteitem_price--administration");
    let deposit_sel = sel(".cassetteitem_price--deposit");
    let gratuity_sel = sel(".cassetteitem_price--gratuity");
    let madori_sel = sel(".cassetteitem_madori");
    let menseki_sel = sel(".cassetteitem_menseki");
    let td_sel = sel("td");

    let id = row
        .select(&id_sel)
        .find_map(|i| i.value().attr("value"))
        .map(str::to_owned)?;

    // The bare "N階" cell is the first <td> that is just a floor.
    let floor = row
        .select(&td_sel)
        .map(text)
        .find(|t| t.ends_with('階') && t.len() <= "10階".len() + 6);

    Some(ScrapedUnit {
        floor,
        layout: row
            .select(&madori_sel)
            .next()
            .map(text)
            .filter(|s| !s.is_empty()),
        area_m2: row
            .select(&menseki_sel)
            .next()
            .and_then(|e| parse_area(&text(e))),
        rent_yen: row
            .select(&rent_sel)
            .next()
            .and_then(|e| parse_yen(&text(e))),
        admin_fee_yen: row
            .select(&admin_sel)
            .next()
            .and_then(|e| parse_yen(&text(e))),
        deposit_yen: row
            .select(&deposit_sel)
            .next()
            .and_then(|e| parse_yen(&text(e))),
        key_money_yen: row
            .select(&gratuity_sel)
            .next()
            .and_then(|e| parse_yen(&text(e))),
        url: format!("https://suumo.jp/chintai/bc_{id}/"),
        source_unit_id: id,
    })
}

/// "東急田園都市線/三軒茶屋駅 歩10分" / "ＪＲ山手線 渋谷駅 徒歩5分"
pub(crate) fn parse_station(s: &str) -> Option<StationAccess> {
    let t = to_halfwidth(s);
    let walk_min = parse_walk_min(&t);
    let head = t.split(['歩', '分']).next().unwrap_or(&t).trim();
    let head = head.trim_end_matches("徒").trim();
    // Split line / station on '/' first, else on the last space.
    let (line, station) = if let Some((l, r)) = head.split_once('/') {
        (l.trim().to_string(), r.trim().to_string())
    } else if let Some(idx) = head.rfind(char::is_whitespace) {
        (
            head[..idx].trim().to_string(),
            head[idx..].trim().to_string(),
        )
    } else {
        (String::new(), head.to_string())
    };
    if station.is_empty() {
        return None;
    }
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
        let html = include_str!("../../tests/fixtures/suumo_setagaya.html");
        let buildings = parse_list(html, 2026);
        assert!(buildings.len() >= 5, "got {}", buildings.len());

        let with_units = buildings.iter().find(|b| !b.units.is_empty()).unwrap();
        assert!(!with_units.name.is_empty());
        assert!(!with_units.address.is_empty());
        let u = &with_units.units[0];
        assert!(u.rent_yen.unwrap_or(0) > 10_000);
        assert!(!u.source_unit_id.is_empty());
        assert!(u.url.starts_with("https://suumo.jp/chintai/bc_"));
    }

    #[test]
    fn station_forms() {
        let s = parse_station("東急田園都市線/三軒茶屋駅 歩10分").unwrap();
        assert_eq!(s.line, "東急田園都市線");
        assert_eq!(s.station, "三軒茶屋駅");
        assert_eq!(s.walk_min, Some(10));

        let s = parse_station("ＪＲ山手線 渋谷駅 徒歩5分").unwrap();
        assert_eq!(s.station, "渋谷駅");
        assert_eq!(s.walk_min, Some(5));
    }
}
