//! Normalising OSM `brand` / `operator` / `name` tags to a canonical chain
//! name, and the supermarket price-tier master.
//!
//! Convenience stores are unconditionally `Expensive`. Supermarkets are looked
//! up here; anything not listed defaults to `Cheap` (the common case for
//! regional chains) — the master only needs to name the exceptions.

use chintai_db::entities::enums::{BrandKind, PriceTier};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrandGuess {
    pub name: String,
    pub kind: BrandKind,
    pub price_tier: PriceTier,
}

/// Canonical convenience-store names keyed by substrings that show up in OSM
/// tags (Japanese and romaji).
const CONVENIENCE: &[(&str, &str)] = &[
    ("セブン-イレブン", "セブン-イレブン"),
    ("セブンイレブン", "セブン-イレブン"),
    ("7-eleven", "セブン-イレブン"),
    ("7-11", "セブン-イレブン"),
    ("ローソン", "ローソン"),
    ("lawson", "ローソン"),
    ("ファミリーマート", "ファミリーマート"),
    ("familymart", "ファミリーマート"),
    ("family mart", "ファミリーマート"),
    ("ミニストップ", "ミニストップ"),
    ("ministop", "ミニストップ"),
    ("デイリーヤマザキ", "デイリーヤマザキ"),
    ("daily yamazaki", "デイリーヤマザキ"),
    ("セイコーマート", "セイコーマート"),
    ("seicomart", "セイコーマート"),
    ("ニューデイズ", "NewDays"),
    ("newdays", "NewDays"),
    ("ポプラ", "ポプラ"),
];

/// (substring, canonical name, price tier).
const SUPERMARKET: &[(&str, &str, PriceTier)] = &[
    ("業務スーパー", "業務スーパー", PriceTier::Cheap),
    ("gyomu", "業務スーパー", PriceTier::Cheap),
    ("オーケー", "オーケー", PriceTier::Cheap),
    ("ok store", "オーケー", PriceTier::Cheap),
    ("okストア", "オーケー", PriceTier::Cheap),
    ("ロピア", "ロピア", PriceTier::Cheap),
    ("ラ・ムー", "ラ・ムー", PriceTier::Cheap),
    ("ラムー", "ラ・ムー", PriceTier::Cheap),
    ("トライアル", "トライアル", PriceTier::Cheap),
    ("ビッグ・エー", "ビッグ・エー", PriceTier::Cheap),
    ("まいばすけっと", "まいばすけっと", PriceTier::Cheap),
    ("肉のハナマサ", "肉のハナマサ", PriceTier::Cheap),
    ("ドン・キホーテ", "ドン・キホーテ", PriceTier::Cheap),
    ("西友", "西友", PriceTier::Cheap),
    ("seiyu", "西友", PriceTier::Cheap),
    ("ライフ", "ライフ", PriceTier::Cheap),
    ("マルエツ", "マルエツ", PriceTier::Cheap),
    ("サミット", "サミット", PriceTier::Cheap),
    ("イオン", "イオン", PriceTier::Cheap),
    ("aeon", "イオン", PriceTier::Cheap),
    ("イトーヨーカドー", "イトーヨーカドー", PriceTier::Cheap),
    // --- expensive ---
    ("成城石井", "成城石井", PriceTier::Expensive),
    ("seijo ishii", "成城石井", PriceTier::Expensive),
    ("紀ノ国屋", "紀ノ国屋", PriceTier::Expensive),
    ("kinokuniya", "紀ノ国屋", PriceTier::Expensive),
    ("クイーンズ伊勢丹", "クイーンズ伊勢丹", PriceTier::Expensive),
    ("queen's isetan", "クイーンズ伊勢丹", PriceTier::Expensive),
    ("明治屋", "明治屋", PriceTier::Expensive),
    ("dean & deluca", "DEAN & DELUCA", PriceTier::Expensive),
    ("北野エース", "北野エース", PriceTier::Expensive),
    (
        "ザ・ガーデン自由が丘",
        "ザ・ガーデン自由が丘",
        PriceTier::Expensive,
    ),
];

fn tag_haystack(tags: &serde_json::Value) -> Vec<String> {
    ["brand", "brand:en", "operator", "name", "name:en"]
        .iter()
        .filter_map(|k| tags.get(*k).and_then(|v| v.as_str()))
        .map(|s| s.to_lowercase())
        .collect::<Vec<_>>()
        .into_iter()
        .chain(
            ["brand", "operator", "name"]
                .iter()
                .filter_map(|k| tags.get(*k).and_then(|v| v.as_str()).map(str::to_owned)),
        )
        .collect()
}

/// Classify a `shop=convenience` / `shop=supermarket` element from its tags.
pub fn classify(shop_kind: BrandKind, tags: &serde_json::Value) -> Option<BrandGuess> {
    let hay = tag_haystack(tags);
    let contains = |needle: &str| hay.iter().any(|h| h.contains(needle));

    match shop_kind {
        BrandKind::ConvenienceStore => {
            for (needle, canon) in CONVENIENCE {
                if contains(needle) {
                    return Some(BrandGuess {
                        name: (*canon).to_owned(),
                        kind: BrandKind::ConvenienceStore,
                        price_tier: PriceTier::Expensive,
                    });
                }
            }
            // Unknown chain / independent shop — still a convenience store.
            let name = tags
                .get("brand")
                .or_else(|| tags.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("その他コンビニ");
            Some(BrandGuess {
                name: name.to_owned(),
                kind: BrandKind::ConvenienceStore,
                price_tier: PriceTier::Expensive,
            })
        }
        BrandKind::Supermarket => {
            for (needle, canon, tier) in SUPERMARKET {
                if contains(needle) {
                    return Some(BrandGuess {
                        name: (*canon).to_owned(),
                        kind: BrandKind::Supermarket,
                        price_tier: *tier,
                    });
                }
            }
            let name = tags
                .get("brand")
                .or_else(|| tags.get("name"))
                .and_then(|v| v.as_str())?;
            Some(BrandGuess {
                name: name.to_owned(),
                kind: BrandKind::Supermarket,
                price_tier: PriceTier::Cheap,
            })
        }
    }
}

/// Carrier slug for a parcel locker / post office element.
pub fn carrier_of(tags: &serde_json::Value) -> String {
    let hay = tag_haystack(tags).join(" ").to_lowercase();
    if hay.contains("pudo") {
        "pudo"
    } else if hay.contains("yamato") || hay.contains("ヤマト") || hay.contains("宅急便") {
        "yamato"
    } else if hay.contains("佐川") || hay.contains("sagawa") {
        "sagawa"
    } else if hay.contains("日本郵便") || hay.contains("郵便局") || hay.contains("japan post")
    {
        "japanpost"
    } else if hay.contains("packcity") || hay.contains("パックシティ") {
        "packcity"
    } else {
        "other"
    }
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn convenience_romaji_and_kana() {
        let g = classify(BrandKind::ConvenienceStore, &json!({"brand": "7-Eleven"})).unwrap();
        assert_eq!(g.name, "セブン-イレブン");
        assert_eq!(g.price_tier, PriceTier::Expensive);

        let g = classify(
            BrandKind::ConvenienceStore,
            &json!({"name": "ローソン 渋谷店"}),
        )
        .unwrap();
        assert_eq!(g.name, "ローソン");
    }

    #[test]
    fn supermarket_tiers() {
        let cheap = classify(BrandKind::Supermarket, &json!({"brand": "業務スーパー"})).unwrap();
        assert_eq!(cheap.price_tier, PriceTier::Cheap);
        let exp = classify(BrandKind::Supermarket, &json!({"brand": "成城石井"})).unwrap();
        assert_eq!(exp.price_tier, PriceTier::Expensive);
        let unknown = classify(
            BrandKind::Supermarket,
            &json!({"name": "地元スーパーやまだ"}),
        )
        .unwrap();
        assert_eq!(unknown.price_tier, PriceTier::Cheap);
    }

    #[test]
    fn carriers() {
        assert_eq!(carrier_of(&json!({"brand": "PUDO"})), "pudo");
        assert_eq!(carrier_of(&json!({"operator": "ヤマト運輸"})), "yamato");
        assert_eq!(carrier_of(&json!({"amenity": "post_office"})), "other");
        assert_eq!(carrier_of(&json!({"name": "郵便局"})), "japanpost");
    }
}
