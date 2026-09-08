# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## フロントエンド（Leptos CSR SPA）+ 型の共有クレート化 + クレート平坦化

### クレート構成の変更

- `crates/*` を廃止しリポジトリ直下へ（`db/` `scraper/` `server/` `web/` `types/`）。
- **`types`（`chintai-types`）新設** — serde だけの純データ型で wasm32 でビルド可能。
  ルート型（`RouteResult` / `Itinerary` / `Leg` / `LatLng` / `Mode` …）、
  エンジンの入出力（`SearchRequest` / `Filters` / `RankSpec` / `SearchResponse` /
  `RankedProperty` …）、地図 DTO（`PoiPoint` / `PoiKind` / `Geocoded`）を集約。
  `scraper` と `server` は再エクスポート、`web` はこれだけに依存。
  `RouteError`（`reqwest::Error` を包む）は `scraper` に残置。

### `web`（`chintai-web`）— Leptos 0.8 CSR

- `cargo-leptos` は当環境で openssl-sys のためビルド不可 → **CSR + wasm-bindgen-cli**
  構成。`web/build.sh [--release]` が wasm をビルドして `web/dist/` に出力
  （`chintai.js` / `chintai_bg.wasm` / `index.html` / `style.css`）。
- コンポーネント: `PlaceEditor`（目的地の追加・ジオコーディング・移動手段・分数）、
  `FilterPanel`（家賃/面積/築年/間取りチップ/コンビニ・スーパー徒歩分/安い系必須/荷物m）、
  `RankingControls`（8 指標の重みスライダー）、`ResultList`/`ResultCard`
  （順位・家賃・間取り・サイト別リンク・**各 place の乗換案内風明細**＝会社/路線/種別/
  駅/時刻/停車数/乗換、徒歩の高低差、最速〜最遅レンジ）、`MapPane`。
- `MapPane` — `index.html` の MapLibre シム（`window.chintaiMap`、OSM ラスタータイル）を
  `wasm-bindgen` 経由で操作。候補ピン（順位色・選択強調）＋目的地ピン＋選択物件から
  各目的地への経路線、周辺 POI レイヤ（`/api/pois`）。
- レスポンシブ: CSS Grid。PC は左パネル＋右地図、〜820px は地図全面＋下部シート。
- 設定（places / filters / ranking）は `localStorage` に自動保存・復元。

### `server`

- 静的配信を追加（`ServeDir` + SPA フォールバック、`CHINTAI_WEB_DIR`）。
- API: `POST /api/search` / `GET /api/pois?bbox=&kinds=` / `GET /api/geocode?q=` /
  `GET /api/health`。`AppState` に DB・ルートプロバイダ・ジオコーダを保持。

### 確認

- `cargo fmt --all --check` / `clippy`（native は `-D warnings`、wasm も `-D warnings`）通過。
- `cargo test --workspace --exclude chintai-web` green（db 6 / scraper 24 / server 6）。
- `web/build.sh` で wasm ビルド成功。
- `chintai-server` を `web/dist` + `chintai.sqlite`（東京・神奈川の POI 済み）で起動 →
  `/` が SPA を返す、`/chintai.js` `/style.css` 200、
  `/api/pois?bbox=渋谷周辺` → 108 件、`/api/geocode?q=渋谷駅` 200 を確認。
  ブラウザでの目視確認は全作業完了後にユーザーへ依頼（UI にバグが残る可能性あり）。

### CI

- `.github/workflows/ci.yml` に wasm ターゲット追加、native と wasm を分けて clippy/build。

### 次段（最後）

- 賃貸物件を東京・神奈川全域で **1 秒 1 リクエスト**でゆっくり取得（`ingest rentals`）。
