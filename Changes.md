# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## `chintai-db`: スキーマ + マイグレーション + リポジトリ関数

SQLite 永続化層の中身を実装した。

### エンティティ（`crates/db/src/entities/`）

- `enums.rs` — `BrandKind` / `PriceTier` / `StoreSource` / `TravelMode` /
  `RouteProviderKind` / `ListingSite`。すべて `snake_case` テキスト保存
  （SQLite ファイルを読みやすく、変種追加で番号がずれない）。
- `brand` — コンビニ / スーパーのチェーンと価格帯（`name` 一意）。
- `store` — ブランドに紐づく実店舗。`(source, external_id)` 一意。座標欠落の
  重複行は後段で近接名寄せ。
- `delivery` — 物流拠点。店舗受取なら `store_id`、PUDO ロッカー等の非店舗は
  `store_id` NULL で座標直持ち。`(carrier, external_id)` 一意。
- `property` / `unit` / `listing` — 建物 / 部屋 / サイト別元データ。
  `unit` は `(property_id, dedup_key)`、`listing` は `(site, site_listing_id)` 一意。
- `railway_line` / `station` / `station_line` — 公共交通ルートの会社・路線・
  駅・駅ランク（`station_line.train_types_json` = その駅に停まる種別）。
- `route_cache` — 丸めた座標＋モードでキー化。全行程を `result_json` に格納
  （徒歩の高低差、公共交通の会社/路線/種別/駅/時刻/停車数、最速〜最遅）。UI の
  乗換案内表示に使う。
- `ingest_run` — 定期更新の実行記録。

### マイグレーション（`m20260908_000001_init`）

- エンティティから `create_table_from_entity` でテーブル生成（型・NULL 可否・
  単一列 UNIQUE を引き継ぐ）。複合 UNIQUE と検索用インデックスはマクロで明示追加。
- 親→子の順で作成（外部キー）。

### リポジトリ関数（`crates/db/src/repo.rs`）

- `upsert_brand` / `upsert_store` / `upsert_delivery` / `upsert_property` /
  `upsert_unit` / `record_listing` / `upsert_railway_line` / `upsert_station` /
  `upsert_station_line` / `route_cache_get` / `route_cache_put` /
  `ingest_run_start` / `ingest_run_finish` / `count`。
- パターンは「`INSERT ... ON CONFLICT DO UPDATE`（原子的）→ 自然キーで `SELECT`
  し直して行を返す」。`exec_with_returning` は使わない —— SQLite の
  `last_insert_rowid()` は接続グローバルで、他テーブルへの挿入が挟まると
  conflict-update が誤った行（または無し）を返すため（実テストで再現・回避を確認）。
- `connect()` はインメモリ URL を検知して接続プールを 1 に固定
  （`:memory:` は接続ごとに別 DB になるため）。

### 確認

- `cargo test -p chintai-db` — インメモリ SQLite で 6 ケース green
  （brand/store/delivery のべき等 upsert、property→unit→listing 連鎖、
  route_cache の座標丸めキー一致、ingest_run ライフサイクル）。
- `cargo fmt --all --check` / `cargo clippy --workspace --all-targets -- -D warnings` 通過。
- Windows 上で実行。
