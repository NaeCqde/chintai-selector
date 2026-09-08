//! Text normalisation for de-duplicating buildings seen on different sites.
//! Same building, different rendering: "Ｐｔｉ Ｂｏｎａｎ．ｓ" vs "Pti Bonan.s",
//! "世田谷区太子堂１" vs "東京都世田谷区太子堂4丁目9-4".

/// Fold full-width ASCII / digits / spaces to half-width and trim.
pub fn to_halfwidth(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '　' => ' ',
            '！'..='～' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

const BUILDING_SUFFIXES: &[&str] = &[
    "マンション",
    "アパート",
    "ハイツ",
    "コーポ",
    "レジデンス",
    "ハウス",
    "ビル",
    "その他",
];

/// Canonical building name: half-width, whitespace collapsed, trailing
/// building-type words removed, lower-cased.
pub fn norm_building_name(s: &str) -> String {
    let mut t = to_halfwidth(s).to_lowercase();
    t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    for suf in BUILDING_SUFFIXES {
        if let Some(stripped) = t.strip_suffix(&suf.to_lowercase()) {
            t = stripped.trim().to_string();
        }
    }
    t.replace(['・', '.', '-', '−', 'ー', '　'], "")
        .replace(' ', "")
}

/// Canonical address: drop the leading prefecture, half-width digits, and
/// collapse 丁目/番地/番/号 + separators into '-'.
pub fn norm_address(s: &str) -> String {
    let mut t = to_halfwidth(s);
    for pref in crate::poi::prefectures::all() {
        if let Some(rest) = t.strip_prefix(pref.name) {
            t = rest.to_string();
            break;
        }
    }
    let mut out = String::new();
    for c in t.chars() {
        match c {
            '丁' | '目' | '番' | '地' | '号' | ' ' | '　' | '−' | 'ー' | '‐' => {
                if !out.ends_with('-') {
                    out.push('-');
                }
            }
            _ => out.push(c),
        }
    }
    out.trim_matches('-').to_string()
}

/// Parse a Japanese money string: "13.5万円" -> 135000, "3,000円" -> 3000,
/// "-" / "なし" / "" -> None.
pub fn parse_yen(s: &str) -> Option<i32> {
    let t = to_halfwidth(s).replace([',', ' ', '円'], "");
    if t.is_empty() || t == "-" || t.contains("なし") {
        return None;
    }
    if let Some(man) = t.strip_suffix('万') {
        let v: f64 = man.parse().ok()?;
        return Some((v * 10_000.0).round() as i32);
    }
    t.parse().ok()
}

/// "25.5m2" / "25.5㎡" / "25.5m²" -> 25.5 (leading number wins).
pub fn parse_area(s: &str) -> Option<f64> {
    let t = to_halfwidth(s);
    let num: String = t
        .trim()
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    num.parse().ok()
}

/// "歩10分" / "徒歩 12 分" -> 10 / 12
pub fn parse_walk_min(s: &str) -> Option<i32> {
    let t = to_halfwidth(s);
    let after = t.split(['歩', '分']).collect::<Vec<_>>();
    for chunk in after {
        let digits: String = chunk.chars().filter(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            return digits.parse().ok();
        }
    }
    None
}

/// "築46年" -> 2026-46, "新築" -> current year. Needs the current year.
pub fn parse_built_year(s: &str, this_year: i32) -> Option<i32> {
    let t = to_halfwidth(s);
    if t.contains("新築") {
        return Some(this_year);
    }
    let digits: String = t
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    let age: i32 = digits.parse().ok()?;
    Some(this_year - age)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_converge() {
        assert_eq!(
            norm_building_name("Ｐｔｉ　Ｂｏｎａｎ．ｓ"),
            norm_building_name("Pti Bonan.s")
        );
        assert_eq!(
            norm_building_name("三軒茶屋ハイツ"),
            norm_building_name("三軒茶屋 ハイツ ")
        );
    }

    #[test]
    fn addresses_converge() {
        assert_eq!(
            norm_address("東京都世田谷区太子堂4丁目9-4"),
            norm_address("世田谷区太子堂４－９－４")
        );
    }

    #[test]
    fn money() {
        assert_eq!(parse_yen("13.5万円"), Some(135_000));
        assert_eq!(parse_yen("3,000円"), Some(3_000));
        assert_eq!(parse_yen("-"), None);
        assert_eq!(parse_yen("なし"), None);
    }

    #[test]
    fn area_and_walk_and_year() {
        assert_eq!(parse_area("25.5m2"), Some(25.5));
        assert_eq!(parse_area("30㎡"), Some(30.0));
        assert_eq!(parse_walk_min("東急田園都市線/三軒茶屋駅 歩10分"), Some(10));
        assert_eq!(parse_built_year("築46年", 2026), Some(1980));
        assert_eq!(parse_built_year("新築", 2026), Some(2026));
    }
}
