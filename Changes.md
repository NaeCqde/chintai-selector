# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## `chintai-scraper`: HTTP 基盤 + ジオコーディング

### `http.rs` — 共有 HTTP クライアント

- クレート全体で 1 つの `reqwest::Client`（gzip、UA、タイムアウト）を
  `HttpClient` で包む。アダプタは `get` / `get_text` / `get_json` だけ使い
  `reqwest` を直接触らない。
- ホスト別レートリミッタ内蔵。既定 1.2 秒間隔、`set_host_interval` で個別上書き
  （Nominatim は 1.1 秒）。`acquire` が次スロットまで `sleep_until` で待つ。

### `geocode.rs` — 「建物名 / 住所」→ `{ name, address, lat, lng, source }`

- `trait Geocoder`（`Ok(None)` = 見つからない / `Err` = バックエンド障害→次を試す）。
- `NominatimGeocoder` — OSM、日本限定・`accept-language=ja`。実運用の主力。
- `GoogleScrapeGeocoder` — `/maps/place/` HTML から地図中心の
  `[zoom,lng,lat]` トリプルを抜く best-effort。consent cookie 無しだと汎用地図を
  返して座標を外すことがあるため **既定では無効**（`default_geocoder` の引数で opt-in）。
- `GoogleApiGeocoder` — `GOOGLE_MAPS_API_KEY` がある時だけ。
- `FallbackGeocoder` — 順に試し、日本の緯度経度範囲に収まる最初の結果を返す。
- `default_geocoder(http, api_key, enable_google_scrape)` で標準チェーンを構築。

### テスト

- `geocode.rs` 内: `first_jp_lat_lng` / `read_float` のパーサ単体 4 件（オフライン、CI で実行）。
- `tests/geocode_live.rs`: Nominatim 実アクセス 2 件。`#[ignore]` なので CI は
  オフラインのまま。`cargo test -p chintai-scraper --test geocode_live -- --ignored` で実行。
  - 手動実行して確認済み: 「東京タワー」→ 35.6584/139.7455（実座標 35.6586/139.7454）、
    住所クエリも東京駅周辺の座標を返す。
  - Google スクレイプは手動実行で座標を外した（広島付近を返した）ことを確認 →
    上記のとおり既定無効の判断は妥当。

### 確認

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- `cargo test --workspace` green（db 6 + scraper parser 4、live は ignored）。
- Windows 上で実行。
