# MTR Link OuDia

MTR API の運転時分を OuDiaSecond の基準列車へ反映するデスクトップアプリケーションです。

## 構成

設計書ではフロントエンドを `desktop/` 配下に置く例を示していますが、このリポジトリは既存のルート SvelteKit と `src-tauri/` 配置を維持します。Rust はルート Cargo Workspace で管理し、`mtr-oudia-domain`、`mtr-oudia-application`、`mtr-oudia-infrastructure`、`src-tauri` をメンバーとします。

## 標準検証

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
npm run check
npm run build
```
