# chintai-selector 仕様

通う場所(職場・学校・よく行く場所)から住まいを選ぶための、個人用の賃貸検索補助サービス。
既存の賃貸サイトから集めた物件のうち、登録した場所すべてへの移動条件を満たすものを、アクセスの速さ・家賃・広さなどで順位づけして地図に出す。
仕様を変えたときは、このファイルも必ず更新する。全体の紹介は `README.md`。

## 構成(Rust ワークスペース)

| クレート | 役割 |
|---|---|
| `types` | serde のデータ型(wasm でも使う)。`PlaceSpec`、`Filters`、`RankSpec`、`SearchRequest`/`SearchResponse`、ルート、地図の型 |
| `db` | SQLite の永続化(SeaORM)。エンティティ、移行(`m20260908_000001_init`、`m20260910_000002_property_station`)、検索用のリポジトリ関数 |
| `scraper` | データ収集。ルート検索、POI(Overpass)、賃貸(SUUMO / HOME'S / at home)、ジオコーディング。`ingest` バイナリ |
| `server` | JSON API、フィルターと順位づけのエンジン、Web の静的ファイルの配信 |
| `web` | Leptos CSR の SPA(wasm32)。MapLibre GL と OSM タイル |

`db` と `scraper` は `server` に依存しない。

## データ(SQLite)

エンティティ: 物件 `property` と部屋 `unit`、掲載 `listing`、店舗 `store` とブランド `brand`(コンビニ・スーパー・価格帯)、宅配 `delivery`、駅 `station` と路線 `railway_line` と `station_line`、物件と最寄り駅 `property_station`、経路のキャッシュ `route_cache`、取り込みの記録 `ingest_run`。
接続先は環境変数 `CHINTAI_DB`(既定 `sqlite://chintai.sqlite?mode=rwc`)。

## サーバー(`chintai-server`)

環境変数: `LISTEN`(既定 `127.0.0.1:3000`)、`CHINTAI_DB`、`CHINTAI_WEB_DIR`(既定 `web/dist`。ここを静的に配信し、見つからなければ `index.html`)、`GOOGLE_MAPS_API_KEY`(任意)。

| メソッド・パス | 内容 |
|---|---|
| `GET /api/health` | `ok` |
| `POST /api/search` | `SearchRequest` を受け、`SearchResponse` を返す。場所が 10 を超える、または `limit` が 500 を超えれば 400。失敗は 500(本文にエラーの文字列) |
| `GET /api/pois?bbox=s,w,n,e&kinds=…` | 範囲内の店舗・宅配・駅。`kinds` は `convenience`/`supermarket`/`delivery`/`station` のカンマ区切り(既定は 4 つとも)。`bbox` の形が違えば 400、面積(度²)が 1 を超える・負・非有限なら 400。スーパーで価格帯が安いものは `CheapSupermarket` |
| `GET /api/geocode?q=…` | 住所・地名を座標にする。見つからなければ `null`。失敗は 502 |

CORS はすべて許可。認証はない。

## 検索(`POST /api/search`)

入力:
- `places`: 通う場所の一覧。各: `label`、`lat`/`lng`、`mode`(完全徒歩 / 車 / 公共交通+徒歩)、`max_minutes`(これを超える物件は落とす)、`kind`(職場・学校 / その他。出発時刻の既定値の選択用)、`departure`(出発の時刻。既定は `Commute` が 8:00、`Other` が 10:00)。
- `filters`: 家賃の上限、間取り、築年数の上限、コンビニ・スーパーまでの徒歩分の上限、安いスーパーの必須、宅配までの距離の上限、ほか。
- `ranking`: 順位づけの項目(`RankKey`)ごとの重みと昇順/降順。
- `limit`: 返す件数。

動作: 条件のうち「すべての場所に上限以内で行ける」ものを残し、フィルターで絞り、順位づけして上位 `limit` 件を返す。経路は `route_cache` を使い、なければルートの提供元に問い合わせる。
出力: 順位づけした物件(各場所への経路つき)と、検討した件数。

## ルートの提供元(`scraper::route`)

Google マップのスクレイプ(合計時間)→ `GOOGLE_MAPS_API_KEY` があれば Google Routes API(会社・路線・種別・駅・時刻の明細)。取れなければ推定(`estimate`、`fallback`)。標高は GSI。

## 収集(`ingest`)

低レートで取り込む(`pacing`)。サブコマンド: `overpass --pref N`(店舗・駅などの POI)、`rentals --suumo … --homes …`(賃貸)、`geocode`(物件の座標の補完)、`google-stations`、`backfill-station-names`。
外部のデータ源: OpenStreetMap(Overpass / Nominatim)、GSI、SUUMO、LIFULL HOME'S、at home、Google マップ。

## セキュリティ上の注意

- 個人用で、既定は loopback だけで待ち受ける。外に出すなら認証を足すこと。認証がないため、届く相手は誰でも検索(ルート検索の外部呼び出し、Google の API の利用)を起こせる。
- `LISTEN` を公開に向けると、`/api/search` が外部サービスへの要求と API キーの課金に使われうる。入力の大きさ(場所の数、`limit`)に上限が必要。
- `/api/geocode` は入力を Nominatim などへ送る。
- エラーの内容がそのまま応答に出る(500 / 502)。
- `GOOGLE_MAPS_API_KEY` は環境変数だけに置き、リポジトリに入れない。
- スクレイプ先の利用規約を守ること。
