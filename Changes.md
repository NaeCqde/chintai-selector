# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## エンジンを実用速度に（推定ルート + 精密化） + 東京/神奈川の POI 完全カバー + 賃貸取得

### 全国 POI → 東京・神奈川を Overpass area クエリで完全投入

- `ingest_prefecture` は `area["ISO3166-2"="JP-NN"]` の 1 クエリで都道府県境ちょうどを
  カバー（名前一致は 東京都 で失敗するため ISO コード）。ミラー全滅時のみタイル分割へ。
- 東京 11974 POI（コンビニ 6919 / スーパー 2565 / 郵便局 1587 / 駅 798）、
  神奈川 5990 POI を投入。合計: brand 1226 / store 14243 / delivery 2653 / station 1210。

### ルート検索を 2 段構えに

- `EstimateProvider` 追加 — 直線距離 × 迂回係数 / モード速度 + 待ち時間の **即時見積り**
  （通信なし・乗換明細なし・`provider: Estimate`）。
- エンジン: (1) 静的 + POI フィルタ → (2) **直線距離の事前フィルタ**
  （その時間で物理的に届かない候補を除外）+ 近い順に上限 `ROUTE_CAP=60` →
  (3) 60 件を `EstimateProvider` で一括評価（数秒）→ ランキング →
  (4) `SearchRequest.refine=true` のときだけ上位 12 件を実プロバイダ（Google スクレイプ→
  鍵があれば API）で精密化（所要時間 + 乗換案内）。`route_cache` 併用。
- `SearchResponse` に `plausible` / `routed` / `capped` を追加。
- 実測: 3574 候補 → 4.3 秒で順位表示（`refine=false`）。

### 賃貸物件の座標

- `rental::ingest_area` は新規物件を**最寄り駅座標 + 徒歩分×80m を建物ハッシュ由来の
  方位へオフセット**した地点に置く（一覧ページに緯度経度がなく、Nominatim は粗い区名を
  ピンできない）。同じ建物は毎回同じ場所に落ちる。`ingest geocode` が残りを補完。

### フロント

- `SearchBar` に「上位の経路を正確に取得（乗換案内・数十秒）」トグル。
- `RouteDetail` は推定ルートに「推定」バッジ。
- 結果サマリに「近い順に上限あり」表示。

### 確認

- `cargo fmt --all --check` / clippy（native `-D warnings` / wasm `-D warnings`）通過。
- `cargo test --workspace --exclude chintai-web` green（db 6 / scraper 24 / server 6）。
- `web/build.sh --release` → wasm 611KB。
- 実データ end-to-end: `POST /api/search`（北千住・公共交通 30 分・家賃 9 万・18㎡ 以上）→
  HTTP 200 / 4.3 秒、あずま 3.5 万 1K 等が順位付きで返る。
- ブラウザ目視は全作業完了後にユーザーへ依頼。

### 進行中（バックグラウンド）

- 東京・神奈川の賃貸を SUUMO 全区市 + HOME'S 23 区で **1 秒 1 リクエスト**取得中
  （`scripts/crawl_rentals.sh`、数時間）。
