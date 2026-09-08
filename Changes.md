# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## `chintai-scraper::route` — ルート検索

`crates/scraper/src/route/` に A→B の所要時間・徒歩高低差・公共交通の詳細を返す
モジュールを追加。

### 型（`types.rs`）

- `RouteResult { mode, provider, duration, distance_m, transit_fastest,
  transit_slowest, walk_ascent_m, walk_descent_m, itineraries }`。
- `Itinerary { duration, depart, arrive, fare_yen, transfers, legs }`。
- `Leg` = `Walk` / `Transit` / `Drive` の enum。`TransitLeg` は会社(`operator`)・
  路線(`line_name`)・種別(`train_type`: 快速/特急…)・乗車/降車駅・時刻・停車数・
  路線色を保持 → UI の乗換案内表示に使う。
- すべて serde 対応で `route_cache.result_json` と往復可能。

### プロバイダ（`trait RouteProvider`）

- `GoogleRoutesApi`（`api.rs`）— `routes.googleapis.com/.../computeRoutes`。
  `GOOGLE_MAPS_API_KEY` 必須。フィールドマスクで transitDetails を取得し、
  会社・路線・駅・時刻・停車数・色を構造化。`computeAlternativeRoutes` で
  最速〜最遅。種別は headsign / 路線名から推定（API が明示しないため）。
- `GoogleMapsScrape`（`scrape.rs`）— `reqwest` のみ・ブラウザ不使用。
  `/maps/dir/.../data=!3e{0|2|3}` の HTML から所要時間を抽出
  （`[900,"15 分"]` 形式と `"NNNs"` トークン）。**徒歩/車は所要時間が答えの全体**。
  **公共交通はスクレイプでは会社/路線などの明細が取れない**ため合計時間のみ返し、
  Fallback が API へエスカレーション。
- `GsiElevation`（`elevation.rs`）— 国土地理院 標高 API（無料・鍵不要・日本）。
  ポリライン or 直線を最大 16 点サンプリングして登り/下り累積を算出。
- `polyline.rs` — Google encoded polyline デコーダ + 間引き。
- `FallbackProvider`（`fallback.rs`）— 順に試し、mode 別に「十分」な結果を採用。
  transit は「明細付き」でないと不十分扱い → 次（API）へ。全滅時はスクレイプの
  合計時間を返す。
- `default_provider(http, api_key)` = スクレイプ→（鍵があれば）API。
- `route_cached(db, provider, from, to, mode)` — `route_cache` を見て無ければ計算＆保存。

### テスト

- オフライン単体: polyline デコード（Google 公式サンプル）・間引き・
  スクレイプの所要時間抽出・日本語時間パース。全て CI で実行。
- `tests/route_live.rs`（`#[ignore]`、手動実行で確認済み）:
  - GSI 標高: 渋谷→恵比寿の 3 点で 登り 6.1m / 下り 0.4m を取得。
  - スクレイプ徒歩: 渋谷→恵比寿 900s（15分）、標高 登り16.6m / 下り10.9m。
  - スクレイプ公共交通: 最速 900s / 最遅 1800s（概算。実用の明細は API 必須）。

### 確認

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- `cargo test --workspace` green（オフライン 14、live は ignored）。
- Windows 上で実行。API 経路は鍵が無いため未実行（型・パースは実装済み）。
