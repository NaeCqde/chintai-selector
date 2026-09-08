# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## `chintai-scraper::poi` — 全国 POI 収集（Overpass）＋ `ingest` バイナリ

### `poi/overpass.rs`

- `Overpass::ingest_bbox(db, bbox)` — OSM Overpass に 1 クエリで
  `shop=convenience` / `shop=supermarket` / `amenity=parcel_locker` /
  `amenity=post_office` / `railway=station` をまとめて取得し、
  `brand` / `store` / `delivery` / `station` に upsert。node は `lat/lon`、
  way/relation は `out center` の中心座標を使用。`external_id` = `type/id`。
- 公開インスタンスが 429/504 を返すことが多いので、
  overpass-api.de → kumi.systems → openstreetmap.fr → private.coffee の順に
  ミラーへフォールバック（各ホスト 4 秒間隔）。
- `ingest_prefecture(code, grid)` — 都道府県 bbox を `NxM` タイルに割って順次取得
  （既定 3x3、Overpass のタイムアウト回避）。

### `poi/brand_master.rs`

- OSM の `brand`/`operator`/`name` タグ（日本語・ローマ字）から正規ブランド名へ正規化。
- コンビニは無条件 `Expensive`。スーパーは価格帯マスタで判定（業務スーパー/OK/
  ロピア/まいばすけっと…=Cheap、成城石井/紀ノ国屋/クイーンズ伊勢丹…=Expensive、
  未知は Cheap）。
- 荷物受取所の `carrier` 推定（pudo / yamato / sagawa / japanpost / packcity）。

### `poi/prefectures.rs`

- 47 都道府県のおおまかな bbox と、bbox をタイル分割する `tiles()`。

### `ingest` バイナリ

- `ingest overpass --pref <1..47> [--grid NxM]` / `--bbox <s,w,n,e>` / `--all`。
- DB は `CHINTAI_DB` env（SeaORM URL）or 既定 `sqlite://chintai.sqlite?mode=rwc`。
- 実行を `ingest_run` に記録（開始/終了/件数）。

### テスト

- オフライン単体 8 件（都道府県テーブル、タイル分割、ブランド正規化、
  価格帯、carrier 推定、Overpass 要素パース、QL 生成）。CI で実行。
- **実ネットワーク実行で確認**: `ingest overpass --bbox 35.655,139.695,35.670,139.710`
  （渋谷周辺）→ コンビニ 73 / スーパー 5 / 郵便局 9 / 駅 7 を SQLite に投入。
  ブランドが `ローソン` `セブン-イレブン` `まいばすけっと` 等に正規化され、
  price_tier も正しく付与されることを DB ダンプで確認。

### 未実装（次段）

- ブランド公式店舗一覧ページのアダプタ（`poi/brands/`）。トレイト設計のみ記載。
- 座標欠落店舗の Google Maps 個別補完（`poi/google_fill`）。
- 駅と路線（`railway_line` / `station_line`）の紐付け。

### 確認

- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- `cargo test --workspace` green（オフライン 22、live は ignored）。Windows 上で実行。
