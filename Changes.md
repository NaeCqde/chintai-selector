# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## 地図にハザードマップ重ね合わせ機能を追加

国土地理院「重ねるハザードマップ」の公開XYZタイル（`disaportaldata.gsi.go.jp`、鍵不要）を
MapLibre のラスターレイヤーとして追加。地図左上に4種類のチェックボックスを表示:

- 洪水浸水想定（`01_flood_l2_shinsuishin_data`、想定最大規模）
- 土砂災害警戒区域（`05_dosekiryukeikaikuiki`急傾斜地／`05_kyukeishakeikaikuiki`土石流／
  `05_jisuberikeikaikuiki`地すべりの3レイヤーを1トグルでまとめて表示・非表示）
- 津波浸水想定（`04_tsunami_newlegend_data`）
- 高潮浸水想定（`03_hightide_l2_shinsuishin_data`）

各パスは実際にタイルを取得して確認済み（箱根＝土砂災害、鎌倉沿岸＝津波/高潮、
江戸川区＝洪水、いずれも該当ハザードのあるタイルで200を確認）。

- `index.html` の MapLibre シムに `setHazard(kind, visible)` を追加。
  レイヤーは初回トグル時に遅延生成、不透明度0.6、POIドット・経路線より下・
  ベース地図より上に挿入（`beforeId: "pois"`）。
- `web/src/maplibre.rs` に `set_hazard` バインディングを追加。
- `web/src/app.rs` に `HazardPanel` コンポーネント（4チェックボックス＋出典表示
  「ハザードマップポータルサイト（国土地理院）」リンク）を追加し、地図左上に配置。
  トグル状態はセッション内のみ（他の検索条件と違い localStorage には保存しない）。

## 確認

- `cargo build -p chintai-web --target wasm32-unknown-unknown` / `clippy -D warnings` 通過。
- 埋め込みJSを `node --check` で構文検証。
- `web/build.sh --release` → `web/dist` に反映確認（index.html/style.css/chintai.js すべてに
  ハザード関連の内容が含まれることを確認）。
- `chintai-server` を起動し `/` `/style.css` の配信を確認（ブラウザでの見た目は未確認）。
- `cargo test -p chintai-db -p chintai-server` 引き続き green（回帰なし）。

## バックグラウンド継続中

- `google-stations`（駅までの実所要時間取得）: 継続稼働中。
