# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## `chintai-scraper::rental` — 賃貸サイトのスクレイパ（SUUMO / LIFULL HOME'S）

### `rental/mod.rs`

- `trait ListingSource` — サイト別アダプタ。`list_page(area, page)` が
  1 ページ分の `ScrapedBuilding`（建物＋`ScrapedUnit` 群）を返す。空 vec = 末尾。
- `ingest_area(db, source, area, max_pages)` — ページを順にたどり、
  建物を **正規化した名前＋住所** で `property` に upsert（SUUMO と HOME'S の
  同一建物を 1 レコードに寄せる）、各部屋を `dedup_key`（階/間取り/面積0.5㎡/賃料1000円）
  で `unit` に upsert、サイト別の元データを `listing` に記録。

### `rental/normalize.rs`

- 全角→半角、建物種別語（マンション/ハイツ…）除去、住所の丁目/番地/号を `-` に統一。
- `parse_yen`（"13.5万円"→135000）/ `parse_area` / `parse_walk_min` /
  `parse_built_year`（"築46年"→西暦）。

### `rental/suumo.rs`

- `.cassetteitem` per 建物、`table.cassetteitem_other tr.js-cassette_link` per 部屋。
- 賃料 `--rent` / 管理費 `--administration` / 敷金 `--deposit` / 礼金 `--gratuity` /
  間取り `.cassetteitem_madori` / 面積 `.cassetteitem_menseki`。
- 部屋 ID = チェックボックス value、URL = `/chintai/bc_<id>/`。ページは `?page=N`。
- SUUMO は素の UA で 200（`HttpClient::new`）。

### `rental/homes.rs`

- `.moduleInner.prg-building` per 建物、`tr.prg-room[data-kykey]` per 部屋。
- 賃料 `.price .num`（万円）、管理費・敷金/礼金（"1ヶ月" は賃料×月数に換算）、
  `td.layout` の "1SK44.81m²" を種別/面積に分割。
- **HOME'S は非ブラウザ UA を 403** → `HttpClient::browser()`（Chrome UA +
  `Accept-Language: ja`）を使用。個人利用（README 参照）。

### `http.rs`

- `HttpClient::browser()` を追加。`get_html_jp()` は UTF-8 優先・失敗時 Shift_JIS
  で日本語ページをデコード（`encoding_rs`）。

### `ingest` バイナリ

- `ingest rentals --suumo <area> --homes <area> [--pages N]`
- `ingest geocode [--limit N]` — 座標 NULL の property を Nominatim で補完。

### テスト

- オフライン単体 16 件（正規化、SUUMO/HOME'S の実 HTML フィクスチャ
  `tests/fixtures/*_setagaya.html` のパース、駅表記、敷礼の月数換算）。CI で実行。
- **実ネットワーク実行で確認**: `ingest rentals --suumo tokyo/sc_setagaya
  --homes tokyo/setagaya-city --pages 1` → SUUMO 20 棟/85 部屋、HOME'S 30 棟/92 部屋
  を取得。名寄せで property 49 / unit 139 / listing 177（同一部屋に 2 リスティング
  = 名寄せヒットあり）。`ingest geocode` も実行、Nominatim で座標補完。

### 既知の制約 / 次段

- 一覧ページの住所は粗い（"世田谷区上北沢1"）ため Nominatim のヒット率が低い。
  詳細ページ（地図座標あり）のスクレイプで改善予定。
- 一覧ページは同スペックの部屋を区別できず `dedup_key` が粗く寄る。詳細ページ pass で解消。

### 確認

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- `cargo test --workspace` green（オフライン 30、live は ignored）。Windows 上で実行。
