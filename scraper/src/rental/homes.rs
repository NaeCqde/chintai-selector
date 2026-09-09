//! LIFULL HOME'S list-page adapter.
//!
//! `area` is the path fragment after `/chintai/`, e.g. `tokyo/setagaya-city`.
//! The URL is `https://www.homes.co.jp/chintai/<area>/list/?page=N`. HOME'S 403s
//! non-browser User-Agents, so this adapter needs an `HttpClient::browser()`.
//! Structure: `.moduleInner.prg-building` per building, `tr.prg-room[data-kykey]`
//! per unit.

use scraper::{ElementRef, Html, Selector};

use super::normalize::*;
use super::suumo::parse_station;
use super::{ListingSource, RentalError, ScrapedBuilding, ScrapedUnit};
use crate::HttpClient;
use chintai_db::entities::enums::ListingSite;

pub struct Homes {
    http: HttpClient,
    this_year: i32,
}

impl Homes {
    /// `http` must be a browser client (`HttpClient::browser`).
    pub fn new(http: HttpClient) -> Self {
        // Pace is tuned in `crate::pacing::HOMES`.
        http.set_host_interval("www.homes.co.jp", crate::pacing::HOMES);
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
impl ListingSource for Homes {
    fn site(&self) -> ListingSite {
        ListingSite::Homes
    }

    async fn list_page(&self, area: &str, page: u32) -> Result<Vec<ScrapedBuilding>, RentalError> {
        let url = format!("https://www.homes.co.jp/chintai/{area}/list/");
        let page_s = page.to_string();
        let html = super::fetch_html(&self.http, &url, &[("page", &page_s)]).await?;
        Ok(parse_list(&html, self.this_year))
    }
}

pub(crate) fn parse_list(html: &str, this_year: i32) -> Vec<ScrapedBuilding> {
    let doc = Html::parse_document(html);
    let item = sel(".moduleInner.prg-building");
    let name_a = sel("a.prg-bukkenNameAnchor");
    let name_span = sel(".bukkenName");
    let spec_row = sel(".bukkenSpec table tr");
    let station_span = sel("span.prg-stationText");
    let room = sel("tbody.prg-roomList tr.prg-room");

    let mut out = Vec::new();
    for b in doc.select(&item) {
        let name = b.select(&name_span).next().map(text).unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        let building_id = b
            .select(&name_a)
            .next()
            .and_then(|a| a.value().attr("data-bid"))
            .unwrap_or("")
            .to_owned();

        let (mut address, mut stations, mut built_year, mut floors) =
            (String::new(), Vec::new(), None, None);
        for tr in b.select(&spec_row) {
            let th = tr.select(&sel("th")).next().map(text).unwrap_or_default();
            let Some(td) = tr.select(&sel("td")).next() else {
                continue;
            };
            if th.contains("所在地") {
                address = text(td);
            } else if th.contains("交通") {
                stations = td
                    .select(&station_span)
                    .filter_map(|s| parse_station(&text(s)))
                    .collect();
            } else if th.contains('築') || th.contains("階") {
                let t = text(td); // "10年 / 3階建" or "新築 / 5階建"
                built_year = parse_built_year(&t, this_year);
                floors = t.split('/').find(|p| p.contains('階')).and_then(digits_i32);
            }
        }

        let units = b.select(&room).filter_map(parse_unit).collect::<Vec<_>>();
        if units.is_empty() {
            continue;
        }

        out.push(ScrapedBuilding {
            source_building_id: if building_id.is_empty() {
                name.clone()
            } else {
                building_id
            },
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
    let kaisuu = sel("li.roomKaisuu");
    let price_num = sel(".price .priceLabel .num, .price .num");
    let price_cell = sel("td.price");
    let layout_cell = sel("td.layout");

    let id = row.value().attr("data-kykey")?.to_owned();
    let url = row
        .value()
        .attr("data-href")
        .unwrap_or("https://www.homes.co.jp/chintai/")
        .to_owned();

    let floor = row
        .select(&kaisuu)
        .next()
        .map(text)
        .filter(|s| !s.is_empty());

    // "12.4" (万円) then "/5,000円" admin then <br> "1ヶ月/1ヶ月/-/-".
    let rent_yen = row
        .select(&price_num)
        .next()
        .map(text)
        .and_then(|n| parse_yen(&format!("{n}万円")));
    let price_text = row.select(&price_cell).next().map(text).unwrap_or_default();
    let admin_fee_yen = price_text
        .split_once('/')
        .and_then(|(_, rest)| parse_yen(rest.split_whitespace().next().unwrap_or("")));
    let (deposit_yen, key_money_yen) = parse_deposit_key(&price_text, rent_yen);

    // "1SK44.81m²" / "1SK 44.81m²" — the <br> between them may or may not
    // survive as whitespace, so split on "digits before m/㎡".
    let layout_text = row
        .select(&layout_cell)
        .next()
        .map(text)
        .unwrap_or_default();
    let (layout, area_m2) = split_layout_area(&layout_text);

    Some(ScrapedUnit {
        floor,
        layout,
        area_m2,
        rent_yen,
        admin_fee_yen,
        deposit_yen,
        key_money_yen,
        source_unit_id: id,
        url,
    })
}

/// From "…/5,000円 1ヶ月/1ヶ月/-/-" pull the 敷金 / 礼金 (first two of the
/// slash group). "Nヶ月" is N × rent; "N万円"/"N円" parse directly.
fn parse_deposit_key(price_text: &str, rent: Option<i32>) -> (Option<i32>, Option<i32>) {
    let Some(group) = price_text
        .split_whitespace()
        .find(|s| s.contains('ヶ') || (s.matches('/').count() >= 2))
    else {
        return (None, None);
    };
    let mut it = group.split('/');
    let dep = it.next().and_then(|s| money_or_months(s, rent));
    let key = it.next().and_then(|s| money_or_months(s, rent));
    (dep, key)
}

fn money_or_months(s: &str, rent: Option<i32>) -> Option<i32> {
    let s = s.trim();
    if s.is_empty() || s == "-" || s.contains("なし") {
        return None;
    }
    if let Some(m) = s.strip_suffix("ヶ月").or_else(|| s.strip_suffix("カ月")) {
        let months: f64 = to_halfwidth(m).parse().ok()?;
        return rent.map(|r| (r as f64 * months).round() as i32);
    }
    parse_yen(s)
}

fn digits_i32(s: &str) -> Option<i32> {
    let d: String = to_halfwidth(s)
        .chars()
        .filter(|c| c.is_ascii_digit())
        .collect();
    d.parse().ok()
}

/// "1SK44.81m²" -> ("1SK", 44.81). Area is the last run of `[0-9.]` that sits
/// just before an m / ㎡; layout is everything before it.
fn split_layout_area(s: &str) -> (Option<String>, Option<f64>) {
    let t = to_halfwidth(s);
    let chars: Vec<char> = t.chars().collect();
    for i in 0..chars.len() {
        if chars[i] != 'm' && chars[i] != '㎡' {
            continue;
        }
        // walk back over "[0-9.]"
        let mut j = i;
        while j > 0 && (chars[j - 1].is_ascii_digit() || chars[j - 1] == '.') {
            j -= 1;
        }
        if j == i {
            continue;
        }
        let area: String = chars[j..i].iter().collect();
        let layout: String = chars[..j]
            .iter()
            .collect::<String>()
            .trim()
            .trim_end_matches(['/', ' ', '　'])
            .to_string();
        return ((!layout.is_empty()).then_some(layout), area.parse().ok());
    }
    let layout = t.split_whitespace().next().map(str::to_owned);
    (layout.filter(|s| !s.is_empty()), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_setagaya_fixture() {
        let html = include_str!("../../tests/fixtures/homes_setagaya.html");
        let buildings = parse_list(html, 2026);
        assert!(buildings.len() >= 3, "got {}", buildings.len());

        let b = buildings.iter().find(|b| !b.units.is_empty()).unwrap();
        assert!(!b.name.is_empty());
        assert!(b.address.contains("世田谷") || !b.address.is_empty());
        assert!(!b.stations.is_empty());

        let u = &b.units[0];
        assert!(u.rent_yen.unwrap_or(0) > 10_000, "rent {:?}", u.rent_yen);
        assert!(u.area_m2.unwrap_or(0.0) > 5.0, "area {:?}", u.area_m2);
        assert!(!u.source_unit_id.is_empty());
    }

    #[test]
    fn deposit_months_convert() {
        let (d, k) = parse_deposit_key("12.4万円/5,000円 1ヶ月/1ヶ月/-/-", Some(124_000));
        assert_eq!(d, Some(124_000));
        assert_eq!(k, Some(124_000));
        let (d, k) = parse_deposit_key("8万円/3,000円 -/-/-/-", Some(80_000));
        assert_eq!(d, None);
        assert_eq!(k, None);
    }
}
