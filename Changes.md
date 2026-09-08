# Changes

このコミットの内容。次のコミットでまっさらにして書き直す。

## ワークスペース初期化

`open-remote-url` からのコピー物を土台に、賃貸選び支援 Web サービス
`chintai-selector` の骨組みを作った。

### 削除

- コピー由来の不要クレート・ファイルを全削除:
  `sender/` `receiver/` `shared/` `client/` `docs/` `scripts/`、
  ルートとサブの `Cargo.lock`、リリース用 `.github/workflows/ci.yml`、`.prettierrc`。

### 追加

- ルート `Cargo.toml` を `[workspace]` に作り直し。`crates/db` `crates/scraper`
  `crates/server` の 3 メンバー。共有バージョンは `[workspace.dependencies]` に集約。
- **`crates/db`（`chintai-db`）** — SeaORM + SQLite の永続化層。今回は `connect` の
  置き場となる `lib.rs` のみ。スキーマ（エンティティ／マイグレーション）は次段。
  scraper・server に依存しない。
- **`crates/scraper`（`chintai-scraper`）** — `reqwest` のみで外部と通信（ブラウザ不使用）。
  共有 HTTP クライアント生成 `http_client()` と、定期更新バイナリ `ingest` の雛形。
  `chintai-db` に依存、server には依存しない。
- **`crates/server`（`chintai-server`）** — Leptos SSR + JSON API + フィルタ/ランキング
  エンジンの置き場。今回は `axum` で `/api/health` を返すだけ。`db` と `scraper` に依存。
- `.gitignore`（`target/`、`*.sqlite*`、`.env`、`cargo-leptos` の `pkg/` など）。
- `.github/workflows/ci.yml` を「`cargo fmt --check` + `clippy -D warnings` + `test`」に置換。
- `git init` ＋ リモート `origin`（`github.com/NaeCqde/chintai-selector`）。

## 確認

- `cargo build --workspace` 成功。
- `cargo fmt --all --check` 通過。
- `cargo clippy --workspace --all-targets -- -D warnings` 警告なし。
- `LISTEN=127.0.0.1:3577 chintai-server` 起動 → `GET /api/health` が `ok` を返すことを確認。
- いずれも Windows 上で実行。
