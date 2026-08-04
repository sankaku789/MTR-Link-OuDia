# MTR-Link-OuDia

Minecraft Transit Railway 4（MTR4）のローカルWeb APIから路線の駅間所要時間・停車時間を取得し、既存のOuDiaSecondファイルへ運転時刻を反映するデスクトップアプリケーションです。

> [!WARNING]
> 本アプリは選択したOuDiaファイルを直接上書きします。重要なファイルは事前にバックアップしてください。

## 使い方

- しばらくお待ちください.....

## プロジェクトの状態

- 開発段階: ベータ版
- MTR4およびOuDiaの実データに合わせて互換性を調整しています。
- 保存前にプレビューを確認し、原本のバックアップを保持することを推奨します。

## 主な機能

- MTR4の`stations-and-routes` APIから路線・駅・所要時間・停車時間を取得
- Windows上でMTR APIの接続先候補を自動検知
- literal loopback URLを使った手動接続
- dimension一覧の取得と切り替え
- OuDiaSecondファイルの文字コード・改行・未知フィールドを保持した解析
- MTR路線とOuDia既存列車の経路照合
- 普通・快速・急行など、OuDiaに定義された列車種別による候補絞り込み
- 上り・下りの駅順、停車駅、通過駅、分岐駅の複数スロットに対応
- 自動照合できない場合の手動駅スロット対応
- MTRの所要時間を使った時刻プレビュー
- `EkiJikoku`の時刻部分のみを更新
- 保存前後の検証と元ファイルの安全な置換

## 対応データ

### MTR

アプリ内部で次のAPIパスをベースURLへ追加します。

```text
/mtr/api/map/stations-and-routes?dimension=<dimension>
```

MTR4の以下のレスポンス形式に対応しています。

- `data`直下に`stations`・`routes`・`dimensions`を持つ形式
- `status`または`code`などのメタデータを含む形式
- メタデータを持たない非包絡形式

駅間時分が不足している未完成路線や、駅数が2未満の路線は変換候補から除外されます。

### OuDia

対応する`FileType`:

- `OuDiaSecond.1.16`
- `OuDiaSecond.1.17`

対応する文字コード:

- UTF-8（BOMあり・なし）
- Windows-31J（CP932）

LF・CRLFの改行を認識し、元のバイト列と変更対象外のフィールドを保持します。

## 対応OS

### Windows

主要な対応環境です。

- MTR APIの自動検知に対応
- Minecraftログと待受TCPポートから候補を探索
- 手動接続に対応
- OuDiaファイルの読込・上書き保存に対応

### Linux

- 手動接続に対応
- OuDiaファイルの読込・上書き保存に対応
- MTR APIの自動検知は未対応のため、画面上の自動検知ボタンは無効になります
- ネイティブLinux上でMTRクライアントと本アプリを実行する構成を想定しています

WSLからWindows上のMTRへ接続する場合、`127.0.0.1`が同じホストを指さない構成があります。本アプリはセキュリティ上LANアドレスを許可しないため、この構成では接続できない場合があります。

### macOS

Tauri用アイコンは生成されていますが、動作検証は行っていません。自動検知はWindows限定です。

## 通信の安全制約

MTR APIの接続先は次の条件に制限されています。

- `http`のみ
- IPv4の`127.0.0.0/8`またはIPv6の`::1`
- literal IPアドレスのみ
- ベースURLのパスは`/`のみ
- query・fragment・ユーザー情報は禁止
- リダイレクトは禁止
- OSや環境のHTTPプロキシを使用しない
- レスポンスサイズは最大8 MiB
- 接続・応答タイムアウトを設定

`localhost`、LAN IP、外部ホスト、HTTPS URL、APIパスを含むURLは拒否されます。

## ファイル保存の安全設計

保存時は次の検証を行います。

1. 読込時のSHA-256と現在の原本を比較
2. 対象列車と駅スロット対応を検証
3. 元バイト列へ`EkiJikoku`時刻パッチを適用
4. 更新後データをOuDiaとして再解析
5. 同一ディレクトリの一時ファイルへ完全書込み・同期
6. 原本を一時退避
7. 検証済みファイルで原本を置換
8. 置換失敗時は原本を復元

読込後に他のアプリから原本が変更された場合は、上書きを拒否します。24時を超える時刻の保存には現在対応していません。

## 技術構成

- Tauri 2
- Rust
- Svelte 5 / SvelteKit
- TypeScript
- reqwest
- デジタル庁デザインシステムのデザイントークン

Rustコードは次のワークスペースに分割されています。

```text
crates/mtr-oudia-domain          ドメインモデル、OuDia解析、経路照合、時刻生成、パッチ
crates/mtr-oudia-application     ユースケース、セッション管理、DTO、Port
crates/mtr-oudia-infrastructure  HTTP、ファイル、設定、待受ポート取得、安全保存
src-tauri                        Tauriコマンドとデスクトップアプリ構成
```

## 開発環境

必要なツール:

- Node.js 20以降
- npm
- Rust stable
- Tauri 2の各OS向けシステム依存関係

依存関係のインストール:

```bash
npm install
```

フロントエンドの開発サーバー:

```bash
npm run dev
```

Tauriアプリの開発起動:

```bash
npm run tauri dev
```

## ビルド

フロントエンドのみ:

```bash
npm run build
```

デスクトップアプリ:

```bash
npm run tauri build
```

LinuxでTauriをビルドする場合は、WebKitGTK・GTK・AppIndicatorなどの開発パッケージが必要です。

## テストと静的検査

```bash
cargo fmt --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
npm run check
npm run build
```

Linux環境でワークスペース全体をビルドする場合、Tauriが要求するGTK/WebKit系システムライブラリを事前に導入してください。

## 既知の制約

- 自動検知はWindowsのみ
- LinuxではAPI URLの手動入力が必要
- macOSは未検証
- MTR側で駅間時分が未計算の路線は変換対象外
- 既存のOuDia列車をテンプレートとして使用するため、空のダイヤへ新規列車を追加することはできない
- 24時を超える時刻は保存できない
- Operation情報は保持または対象列車からの削除を選択する必要がある
- OuDiaファイルは直接上書きされる

## デザイン

UIはデジタル庁デザインシステムの考え方と`@digital-go-jp/design-tokens`を参照しています。実装用の参照プロファイルは[`DESIGN.md`](./DESIGN.md)にあります。

## ライセンス

MIT License
