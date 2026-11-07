# gpui-poc

MTR-Link-OuDia の UI を GPUI で書き直せるかを判断するための隔離 PoC。

- ルート workspace には含めない（`[workspace]` を空で宣言）。
- 本体の `crates/*` には依存しない。必要になった段階で path 依存を追加する。

## 実行

```sh
cd experiments/gpui-poc
cargo run
```

## 評価したいこと

1. API の手触り（状態変更 → `cx.notify()` → 再描画）
2. 日本語テキスト入力（IME）— 標準部品はなく、`EntityInputHandler` の実装が必要
3. フォント描画とレイアウト
4. Windows 実機での起動・DPI・ファイルダイアログ
5. ビルド時間とバイナリサイズ

## 既知の制約

- Linux では `gpui 0.2.2 → gpui_http_client → zed-async-tar → xattr 0.2.3` が
  `libc::ENOATTR` 非互換でコンパイルできない（2026-11 時点）。
  この PoC では `vendor/xattr` に `ENODATA` へ修正したコピーを置き、
  `[patch.crates-io]` で差し替えている（Windows ビルドには影響しない）。
- `cargo check --target x86_64-pc-windows-msvc` は `ring` が `lib.exe` を要求するため
  Linux 上では不可。Windows 実機または Windows CI で検証する。
- GPUI には標準のテキスト入力部品がない。IME 対応には `EntityInputHandler` の実装が必要。

## 現状

- Milestone 1: ウィンドウ + ラベル + クリックで状態更新するボタン
