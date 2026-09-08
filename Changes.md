# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## `chintai-server::engine` — フィルタ / ランキングエンジン + `/api/search`

### `engine/types.rs`

- `SearchRequest { places, filters, ranking, limit }`（すべて serde、後で Leptos
  サーバー関数境界をそのまま渡せる）。
  - `PlaceSpec { label, lat, lng, mode(walk/car/transit), max_minutes }`。
  - `Filters` = 家賃上限 / 面積下限 / 間取り許可リスト / 築年数上限 /
    コンビニ・スーパーまでの徒歩分上限 / 安い系スーパー必須 / 荷物拠点まで m。
  - `RankSpec { key, weight, ascending }`。`RankKey` = AccessTotal / AccessWorst /
    Rent / Area / RentPerArea / NearestSuper / NearestConveni / BuildYear。
- `SearchResponse { results, considered, matched }`。
  `RankedProperty` は best_unit / other_units / 各 place の `PlaceAccess`
  （所要分・上限内か・**フルの `RouteResult`**＝乗換案内表示用）・
  最寄りコンビニ/スーパー/安い系スーパー/荷物拠点までの距離。
  `UnitView.sources` に SUUMO/HOME'S のサイト別 URL。

### `engine/mod.rs` — 処理

1. `units_with_geocoded_property` を全件ロード → 家賃/面積/間取り/築年で静的フィルタ。
2. `stores_with_brand` / `deliveries_with_coords` から最寄り POI を直線距離で算出
   （徒歩 80m/分換算）。POI 徒歩分フィルタで足切り。
3. 各 place を `route_cached`（`route_cache` 併用、`buffer_unordered(6)` で並行）で計算、
   全 place が `max_minutes` 以内の物件だけ残す。
4. 各ランキング指標を 0..1 に正規化 → 重み付き合算 → ソート → 上位 `limit`。
   ランキング未指定時は AccessTotal(1.0) + Rent(0.5)。

### `engine/geo.rs`

- `haversine_m` / `walk_minutes`（80m/分）。

### `chintai-db`

- 読み取りモデル追加: `units_with_geocoded_property` / `stores_with_brand` /
  `deliveries_with_coords` / `stations_all` / `listings_for_unit`。

### `chintai-server`

- `POST /api/search`（`SearchRequest` JSON → `SearchResponse` JSON）。
  起動時に DB 接続＋`route::default_provider`（スクレイプ→鍵があれば API）を構築。

### テスト

- `engine/tests.rs` — インメモリ SQLite に合成データ（近い物件 A・遠い物件 B、
  コンビニ/安い系スーパー、複数ユニット）＋直線距離モックの `RouteProvider` で:
  - 時間上限で遠い物件が落ちる / best_unit が最安ユニット / 最寄り POI 距離
  - 家賃・間取りフィルタ / コンビニ徒歩分フィルタ
  - Area ランキングで広い物件が 1 位 / route 明細が結果まで伝わる
  6 ケース green。`geo` の haversine 1 ケース。

### 確認

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- `cargo test --workspace` green（オフライン 37）。
- **実データ end-to-end**: 世田谷の賃貸 + 周辺 POI(780 店舗/143 拠点/64 駅) を
  ingest した SQLite に対し `chintai-server` を起動、`POST /api/search`
  （渋谷勤務・公共交通 40 分以内・家賃 20 万以下）→ HTTP 200。
  aiflat明大前(1DK ¥145000, 12 ユニット、SUUMO 重複リスティングを名寄せ) 等が
  順位付きで返り、最寄りコンビニ 204m / スーパー 408m / 荷物拠点 334m、
  各 place の所要 15 分・route 明細（スクレイプは transit 合計のみ、明細は API 鍵必要）を確認。
- Windows 上で実行。
