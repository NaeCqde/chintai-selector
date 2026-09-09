# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## スクレイピング速度をサイトごとに設定できる仕組みに変更

`scraper/src/pacing.rs` を新設し、サイト（ホスト）ごとのリクエスト間隔を1箇所に集約。
各アダプタのコンストラクタが自分のホスト分だけを `HttpClient::set_host_interval` で
適用する（既存の Nominatim/Google Maps/GSI標高/Overpass のパターンを、SUUMO/HOME'S/
AtHome にも展開した形）。自動でリトライ間隔を調整するような仕組みは意図的に作らず、
ブロックされたら該当サイトの定数を人間が下げて再ビルドする、という運用にしている。

- 全サイトのデフォルトを1.2秒→**0.3秒**に変更（ユーザー指示）。
  - SUUMO (`suumo.jp`) / HOME'S (`www.homes.co.jp`) / AtHome (`www.athome.co.jp`)
  - Google Maps (`www.google.com`) — 物件→駅の一括事前計算と、検索時にエンジンが
    その場で叩く物件→ユーザー登録地点のライブルート取得の両方に効く
  - Nominatim (`nominatim.openstreetmap.org`) / GSI標高 (`cyberjapandata2.gsi.go.jp`)
    / Overpass（各ミラー）
  - AtHome は既知のbot対策（パズル認証）があるため他サイトと別定数
    （`pacing::ATHOME`）に分けてあり、詰まったらそこだけ下げられる。
- `ingest rentals` の `--interval-ms` フラグは廃止（後方互換のため引数は読み捨てるが
  もう何もしない）。サイトの速度は呼び出し時の引数ではなく `pacing.rs` の定数で決まる。
- `scripts/crawl_rentals.sh` から `--interval-ms` 指定を削除、コメントを更新。

## 確認

- `cargo build --workspace` / `cargo clippy --workspace --exclude chintai-web -D warnings` 通過。
- `cargo test -p chintai-scraper -p chintai-db -p chintai-server` 35件すべてgreen。
- `ingest google-stations --limit 20` を実測: 1件あたり約4.5秒（ボトルネックは
  Google Mapsそのものではなく、経路1本につき最大16回呼ぶGSI標高サンプリング）。
  詰まったりブロックされたりする様子はなし。

## バックグラウンド継続中

- `google-stations`（駅までの実所要時間取得）: 新しいペース設定で再起動して継続稼働中。
