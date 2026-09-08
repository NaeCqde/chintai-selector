# chintai-selector

通う場所から住まいを選ぶための賃貸検索補助サービス（ホテル業界の Trivago に相当）。

職場・学校・よく行く場所を複数登録し、それぞれに移動手段（完全徒歩 / 車 / 公共交通＋徒歩）と
所要時間の上限を設定する。既存の賃貸サイトから集めた物件のうち、すべての条件を突破したものを
アクセスの速さ・家賃・広さなどで順位付けし、地図に候補と目的地を重ねて表示する。

## 構成

Rust ワークスペース。

| クレート | 役割 | 依存 |
|---|---|---|
| `crates/db` (`chintai-db`) | SQLite 永続化層（SeaORM）。スキーマとリポジトリ | なし |
| `crates/scraper` (`chintai-scraper`) | データ収集。ルート検索・POI・賃貸スクレイパ。`reqwest` のみ（ブラウザ不使用）。`ingest` バイナリ | `db` |
| `crates/server` (`chintai-server`) | Leptos SSR + JSON API + フィルタ/ランキングエンジン | `db`, `scraper` |

`db` と `scraper` は `server` に依存しない。

## 開発

```sh
cargo build --workspace
cargo test --workspace
cargo run -p chintai-server            # http://127.0.0.1:3000
```

外部データ源: OpenStreetMap（Overpass / Nominatim）、各ブランド公式の店舗一覧、
SUUMO・LIFULL HOME'S、ルート検索は Google Maps スクレイプ→ Google Routes API フォールバック。
地図表示は MapLibre GL + OSM タイル。
