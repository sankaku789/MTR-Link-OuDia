# gpui-poc

MTR-Link-OuDia の UI を GPUI で書き直せるかを判断するための隔離 PoC。

- ルート workspace には含めない（`[workspace]` を空で宣言）。
- `crates/mtr-oudia-application` と `crates/mtr-oudia-infrastructure` を path 依存で参照する
  （既存ロジックを GPUI から呼べるかの検証用）。

## 実行

IME / テキスト入力の検証:

```sh
cd experiments/gpui-poc
cargo run
```

async bridge の検証（既存 `ReqwestMtrApiClient` を呼ぶ）:

```sh
# Windows (PowerShell)
$env:MTR_ENDPOINT = "http://127.0.0.1:49182"
$env:MTR_DIMENSION = "0"   # 省略可
cargo run --bin fetch_poc
```

`MTR_ENDPOINT` は MTR API のリッスンポート。MTR-Link-OuDia 本体の Step 1 で
検出された接続先を使う。

## 評価したいこと

1. API の手触り（状態変更 → `cx.notify()` → 再描画）
2. 日本語テキスト入力（IME）— 標準部品はなく、`EntityInputHandler` の実装が必要
3. フォント描画とレイアウト
4. Windows 実機での起動・DPI・ファイルダイアログ
5. ビルド時間とバイナリサイズ

## 既知の制約

- **Windows の IME 切り替え（2026-11 時点）**: 公開 crate の `gpui 0.2.2`（2025-10-22）には
  Zed PR #41259（2025-11-05, Windows の国際キーボード/IME 対応）が入っておらず、
  半角/全角キーで日本語入力に切り替えられない（Zed issue #40592）。
  Alt+半角/全角、Win+Space、タスクバーからの切り替えは動作する。
  この PoC では修正コミット `2ead8c42` の gpui を git 依存で使用する。
- Linux では `gpui → gpui_http_client → zed-async-tar → xattr 0.2.3` が
  `libc::ENOATTR` 非互換でコンパイルできない（2026-11 時点）。
  この PoC では `vendor/xattr` に `ENODATA` へ修正したコピーを置き、
  `[patch.crates-io]` で差し替えている（Windows ビルドには影響しない）。
- `cargo check --target x86_64-pc-windows-msvc` は `ring` が `lib.exe` を要求するため
  Linux 上では不可。Windows 実機または Windows CI で検証する。
- GPUI には標準のテキスト入力部品がない。IME 対応には `EntityInputHandler` の実装が必要。
- gpui 0.2.2 の `examples/input.rs` には IME の composition 位置ずれ・コピー時 panic の
  既知 issue がある（Zed #64389）。

## 現状

- Milestone 1: ウィンドウ + ラベル + クリックで状態更新するボタン
- Milestone 2: 公式 `input.rs` をベースにテキスト入力を追加。入力中のキーストロークと
  確定後の内容を画面に表示するので、IME の composition 挙動を実機で観察できる
  （`unicode-segmentation` を使用）
- Milestone 3: 0.2.2 の半角/全角キー不具合を避けるため、gpui を修正コミット
  `2ead8c42` の git 依存へ切り替え（`input.rs` は 0.2.2 と同一で API 互換）
- Milestone 4: `src/bin/fetch_poc.rs` で既存 `ReqwestMtrApiClient::fetch_snapshot` を
  呼ぶ async bridge を追加。tokio ランタイムを専用スレッドで動かし、結果を
  oneshot で GPUI の executor に返して路線一覧を再描画する。
