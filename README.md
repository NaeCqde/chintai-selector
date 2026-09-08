# chintai-selector

通う場所から住まいを選ぶための賃貸検索補助サービス（ホテル業界の Trivago に相当）。

職場・学校・よく行く場所を複数登録し、それぞれに移動手段（完全徒歩 / 車 / 公共交通＋徒歩）と
所要時間の上限を設定する。既存の賃貸サイトから集めた物件のうち、すべての条件を突破したものを
アクセスの速さ・家賃・広さなどで順位付けし、地図に候補と目的地を重ねて表示する。個人用・非公開。

## 構成

Rust ワークスペース。

| クレート | 役割 | 依存 |
|---|---|---|
| `types` (`chintai-types`) | serde だけのデータ型（wasm 可）。API と WASM が共有 | なし |
| `db` (`chintai-db`) | SQLite 永続化層（SeaORM）。スキーマとリポジトリ | なし |
| `scraper` (`chintai-scraper`) | データ収集。ルート検索・POI(Overpass)・賃貸(SUUMO/HOME'S)。`reqwest` のみ。`ingest` バイナリ | `db`, `types` |
| `server` (`chintai-server`) | JSON API + フィルタ/ランキングエンジン + WASM の静的配信 | `db`, `scraper`, `types` |
| `web` (`chintai-web`) | Leptos CSR SPA（wasm32）。MapLibre GL + OSM タイル | `types` |

`db` と `scraper` は `server` に依存しない。

## 開発

```sh
cargo build --workspace --exclude chintai-web
cargo test  --workspace --exclude chintai-web

# データ投入（個人用・低レート）
export CHINTAI_DB="sqlite://chintai.sqlite?mode=rwc"
cargo run -p chintai-scraper --bin ingest -- overpass --pref 13   # 東京の POI
cargo run -p chintai-scraper --bin ingest -- overpass --pref 14   # 神奈川の POI
cargo run -p chintai-scraper --bin ingest -- rentals --suumo tokyo/sc_setagaya --homes tokyo/setagaya-city
cargo run -p chintai-scraper --bin ingest -- geocode              # 物件座標を補完

# フロント（要 wasm-bindgen-cli、wasm32 ターゲット）
web/build.sh --release          # -> web/dist

# 起動
cargo run -p chintai-server     # http://127.0.0.1:3000
```

`GOOGLE_MAPS_API_KEY` を設定すると、公共交通ルートの明細（会社・路線・種別・駅・時刻）が
Google Routes API から取得される（未設定時はスクレイプの合計時間のみ）。

外部データ源: OpenStreetMap（Overpass / Nominatim / GSI 標高）、SUUMO・LIFULL HOME'S、
ルート検索は Google Maps スクレイプ →（鍵があれば）Google Routes API。地図は MapLibre GL + OSM タイル。
