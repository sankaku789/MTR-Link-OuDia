# MTR API → OuDiaSecond 基準運転時分更新ソフト 設計書

- 文書状態: Draft 3.2（Codex実装向け）
- 改訂内容: Draft 3.1の仕様を維持し、Codex向け実装契約、非交渉の不変条件、検証コマンド、Issue単位の受け入れ条件を追加
- アプリ形態: 独立したデスクトップGUIソフト
- MTRデータ取得元: MTRクライアントのlocalhost API
- 出力対象: 既存のOuDiaSecond `.oud2`
- サンプル確認対象: `Shinkyu.oud2`
- 初期正式対応OS: Windows 10/11 x64
- 後続対応候補: Linux x86_64
- 正式版の処理単位: 単一MTR路線・片方向・既存基準列車1本の時刻置換
- 基準始発時刻: `10:00:00` 固定


---

## 0. Codex向け実装ガイド

この章は、Codexまたは他のコーディングエージェントが本設計書から実装を進める際の実装契約である。以降の各章が詳細仕様であり、この章は読み方、作業単位、検証方法を定義する。

### 0.1 仕様の優先順位

矛盾が見つかった場合は、次の順序で判断する。

1. 「仕様確定済み事項」
2. 「非交渉の不変条件」
3. 各機能章の明示的な要求
4. 「正式版完了条件」
5. 「実装フェーズ」
6. コード内コメント

設計書にない挙動を推測して追加してはならない。情報不足の場合は、元ファイルを変更しない安全側の失敗として実装し、未確定事項をテスト名、エラー、またはIssueへ明示する。

### 0.2 要求語

本設計書では、次の意味で要求語を使用する。

- **MUST**: 実装・テストともに必須。満たさなければ完了ではない
- **SHOULD**: 原則として実装する。省略時は理由を記録する
- **MAY**: 任意。正式版の完了条件には含めない

既存の日本語記述で「する」「行う」「禁止する」と断定されているものは、原則としてMUSTとして扱う。

### 0.3 非交渉の不変条件

以下は、実装中の都合で変更してはならない。

1. 基準始発発時刻は `10:00:00` 固定である
2. 時間はミリ秒で累積し、OuDiaへ出力するときだけ秒へ丸める
3. DomainはHTTP、ファイル、OS API、Tauriへ依存しない
4. ApplicationはInfrastructureの具象型へ依存しない
5. 元の `.oud2` は既定で上書きしない
6. 無変更保存は可能な限り入力バイト列と完全一致させる
7. 時刻変更時は許可されたSourceRange以外のバイト列を変更しない
8. 未知の `EkiJikoku`、FileType、構文を推測して書き換えない
9. 保存後は再解析と差分範囲検証の両方に成功しなければ成功扱いにしない
10. GUIはOuDia解析、経路照合、時刻計算を実装しない
11. CLIは作成しない
12. Linux対応のためにWindows版のDomain/Applicationを書き換える構造にしない

### 0.4 Codexの作業単位

1回の実装タスクは、原則として次の条件を満たす小さな単位に分ける。

- 主対象は1つのcrateまたは1つのユースケース
- 公開型・traitを変更する場合は影響範囲を同じタスク内で更新する
- 無関係なリファクタリングを混ぜない
- 実装と同時に正常系・異常系テストを追加する
- fixtureが不足する場合は、実データを推測せず最小fixtureを追加する
- 未確定仕様を仮実装で埋めない

各タスクの完了報告には、次を含める。

```text
実装した内容
変更した主要ファイル
追加・更新したテスト
実行した検証コマンドと結果
未検証または未確定の事項
```

### 0.5 共通Definition of Done

各Issueまたは実装タスクは、次をすべて満たしたときに完了とする。

- 対象章の受け入れ条件を満たす
- 公開APIに必要なRustdocまたは説明コメントがある
- 正常系と少なくとも1つの異常系テストがある
- 既存テストを壊していない
- `cargo fmt`、`cargo clippy`、`cargo test`が成功する
- GUI変更時はTypeScript/Svelteの型検査とビルドが成功する
- OS依存コードは適切な `cfg` 境界内にある
- ログへOuDia本文や過剰な個人情報を出力しない
- 設計書と異なる判断をした場合は、その理由を明記する

### 0.6 標準検証コマンド

リポジトリのスクリプトが未整備の段階では、以下を標準とする。

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace

cd desktop
npm ci
npm run check
npm run build
```

Tauriの配布ビルドは、P12までは全タスクの必須条件にしない。TauriコマンドやGUI統合を変更した場合は、開発ビルドまたは該当する統合テストを追加で実行する。

### 0.7 実装開始前に読む章

| 作業対象 | 必読章 |
|---|---|
| Domain全般 | 6、9、17、20〜32、37〜38、41 |
| MTR API | 10〜19、39〜41 |
| OuDia parser/editor | 20〜32、36〜41、44 |
| Windowsポート検出 | 11〜16、39〜41 |
| Tauri/Svelte GUI | 8、33〜35、39〜41 |
| リリース | 35〜45 |

---

## 1. 目的

Minecraft Transit Railway（以下MTR）のシステムマップAPIから、選択した路線の以下の情報を取得する。

- 駅の並び
- 各駅の停車時分
- 各駅間の運転時分

MTR APIは、この用途では絶対的な列車時刻を返すものとして扱わない。取得した運転時分・停車時分を利用し、始発駅の発時刻を `10:00:00` に固定して、既存のOuDiaSecondファイル内にある基準列車の発着時刻を再構築する。

最終的な利用手順は次のとおり。

```text
MinecraftでMTRのマルチサーバーへ接続
  ↓
MTRクライアントのローカルWebサーバーが起動
  ↓
独立ソフトがlocalhost上のMTR APIを自動検出
  ↓
MTR路線を選択
  ↓
既存の.oud2を選択
  ↓
OuDia内の対応経路・基準列車を選択
  ↓
10:00:00を起点にMTR時分から発着時刻を生成
  ↓
変更内容をプレビュー
  ↓
別名の.oud2として安全に保存
```

---

## 2. 確定した基本方針

本ソフトの基本方針を次のとおり確定する。

- Minecraft Modではなく独立したデスクトップソフトとする
- GUIはTauri 2とSvelteで構築する
- GUIの視覚設計には `@digital-go-jp/design-tokens` を使用する
- ファイル解析・照合・時刻生成・保存はRustで実装する
- CLIは作成しない
- Rust処理はDomain、Application、Infrastructureへ分離する
- OS非依存ロジックからOS依存処理を参照させない
- 初期正式リリースはWindows x64を対象とする
- Linux対応を後付けできる構造にする
- 「Unix全般対応」は目標にしない
- MTRのマルチサーバー接続中も、クライアントlocalhost APIを経由してデータを取得する
- クライアント側APIポートは固定値と仮定しない
- APIは駅間運転時間と停車時間をミリ秒で取得する
- 基準始発時刻は `10:00:00` 固定とし、GUIから変更させない
- 既存のOuDia列車を経路テンプレートとして使用する
- 着発時刻以外の情報は可能な限り元バイト列を保持する
- 元ファイルは既定で上書きしない
- 保存後に再解析・構造比較・差分範囲検証を行う

---

## 3. 結論となる構成

```text
MTR Client localhost API
  ↓ HTTP / JSON
Tauri Desktop
  ↓ Tauri Command
Application Use Case
  ↓
Domain Logic
  ├─ MTRデータ正規化後モデル
  ├─ OuDia解析モデル
  ├─ 経路照合
  ├─ 10時起点の時刻生成
  ├─ EkiJikoku更新計画
  └─ 差分検証規則

Infrastructure
  ├─ HTTPクライアント
  ├─ ファイル入出力
  ├─ 原子的保存
  ├─ 設定保存
  └─ OS別待受ポート列挙
```

サーバー側Mod、クライアント専用Mod、Mixin、Minecraft内部クラスへの直接アクセスは使用しない。

---

## 4. 独立ソフトにする理由

### 4.1 OuDia編集が主処理である

処理の中心はMinecraft内の表示ではなく、次のデスクトップ操作である。

- `.oud2` の選択
- 経路候補の比較
- 駅対応の確認
- 発着時刻のプレビュー
- 変更前後の差分確認
- Operation情報の扱い選択
- 別名保存
- エラー詳細表示

### 4.2 MTRデータを公開HTTP API経由で取得できる

使用するエンドポイントは次である。

```text
GET http://127.0.0.1:{port}/mtr/api/map/stations-and-routes?dimension={dimension}
```

マルチサーバー接続時、クライアント側localhost Webサーバーは受信したリクエストをMinecraftパケットで接続先サーバーへ転送し、サーバー側MTRの応答をクライアントへ返す。

### 4.3 MTR内部実装への依存を限定できる

本ソフトが直接依存するものは次に限定する。

- localhost APIのエンドポイント
- JSON応答包絡
- 路線・駅・運転時分のJSON構造

Minecraft内部クラスやFabric APIを本ソフトから直接参照しない。

### 4.4 API取得後はオフラインで作業できる

API応答を正規化したスナップショットをメモリに保持する。取得後はMinecraftやサーバー接続が終了しても、以下の作業を継続できる。

- OuDia読込
- 経路候補選択
- 駅対応修正
- 時刻プレビュー
- `.oud2` 保存

---

## 5. 対象範囲

### 5.1 正式版で対応するもの

- Windows 10/11 x64向け独立GUIソフト
- MTR 4.x系のlocalhost System Map API
- マルチサーバー接続中のクライアントlocalhost API
- APIポート自動検出
- API URL手動入力
- dimension指定・選択
- MTR路線一覧の取得
- 単一MTR路線の選択
- 駅間運転時分の取得
- 駅停車時分の取得
- 既存 `.oud2` の読み込み
- `OuDiaSecond.1.16` と `OuDiaSecond.1.17` の正式読書き対象化
- `KijunDiaIndex` による基準ダイヤ特定
- 既存基準列車から経路テンプレート抽出
- 同名駅
- デルタ線
- 分岐線
- `Kudari` / `Nobori` の方向判定
- 列車種別の手動選択
- 対象基準列車の時刻置換
- `10:00:00` 固定の時刻生成
- 保存前プレビュー
- Operation情報の保持・削除選択
- 別名保存
- 保存後の再解析検証
- 変更許可範囲外のバイト列不変検証

### 5.2 初期正式リリースでは対象外

- Linuxの正式サポート
- macOSの正式サポート
- FreeBSD等の汎用Unix対応
- Minecraft内部データへの直接アクセス
- Fabric Mod
- サーバー側Mod
- CLI
- 複数MTR路線の一括反映
- 複数OuDiaファイルの一括処理
- 新しい `.oud2` の完全生成
- OuDiaSecond本体の自動操作
- 複雑な環状線の完全自動判定
- ダイヤ全列車の自動生成
- Operation情報の完全再計算
- 未対応FileTypeへの保存

### 5.3 後続候補

- Linux x86_64版
- AppImage等のLinux配布
- macOS対応調査
- 複数路線の一括更新
- 新規基準列車追加
- Operation情報の再構築

---

## 6. アーキテクチャ原則

### 6.1 依存方向

```text
desktop
   ↓
application
   ↓
domain

infrastructure
   ↓
applicationが定義するPortを実装
   ↓
domainの型を利用
```

禁止する依存方向は次のとおり。

```text
domain → application
domain → infrastructure
domain → desktop

application → infrastructure
application → desktop

infrastructure → desktop
```

### 6.2 Domainの純粋性

Domainには以下を置かない。

- HTTP通信
- ファイル読書き
- OS API
- Tauri
- GUI用状態
- 設定ディレクトリ取得
- 現在時刻取得

Domainは、入力値から出力値を生成する決定的な処理を中心とする。

### 6.3 Applicationの役割

Applicationはユーザー操作単位のユースケースを定義する。

- API検出
- MTRスナップショット取得
- OuDiaファイル検査
- 経路候補抽出
- プレビュー生成
- 安全保存

外部処理はPort traitを通じて利用する。

### 6.4 Infrastructureの役割

Infrastructureは副作用と外部依存を担当する。

- HTTP
- OS別待受ポート列挙
- ファイル入出力
- 一時ファイル
- 原子的リネーム
- 設定保存
- ログ出力

### 6.5 Desktopの役割

DesktopはTauriコマンド、DI構築、Svelte GUIを担当する。OuDia解析や時刻計算をTauriコマンド内へ直接実装しない。

---


### 6.6 層別の変更責任

Codexは、機能を追加するときに処理を置く層を次の表で決定する。

| 関心事 | 所属 | 他層へ漏らしてはいけないもの |
|---|---|---|
| 時刻、駅スロット、経路、差分規則 | Domain | HTTP型、PathBuf操作、Tauri DTO |
| ユースケース順序、Port trait、業務エラー | Application | reqwest、Windows API、Svelte状態 |
| HTTP、ファイル、設定、OS API | Infrastructure | GUI画面状態、業務判断の再実装 |
| Tauri command、DTO変換、画面状態 | Desktop | OuDia構文解析、時刻計算、保存規則 |

同じ業務規則を複数層へ複製しない。DesktopまたはInfrastructureでDomain規則が必要になった場合は、Domainの公開APIを呼び出す。

### 6.7 公開境界の安定性

- Domainの公開型は、GUI表示都合ではなく業務概念で命名する
- Tauri向けDTOはDesktop内に置き、Domain型へ `serde` 属性を過剰に追加しない
- ApplicationのPort traitはユースケースが必要とする最小操作だけを公開する
- Infrastructure固有のエラーはApplication境界で業務エラーへ変換する
- 1つのTauri commandは原則として1つのApplication use caseへ対応させる

## 7. ディレクトリ構造

Cargo Workspaceを使用する。

```text
mtr-oudia/
├─ Cargo.toml
├─ Cargo.lock
├─ rust-toolchain.toml
├─ README.md
├─ LICENSE
│
├─ crates/
│  ├─ mtr-oudia-domain/
│  │  ├─ Cargo.toml
│  │  ├─ src/
│  │  │  ├─ lib.rs
│  │  │  ├─ mtr/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ route.rs
│  │  │  │  ├─ station.rs
│  │  │  │  └─ snapshot.rs
│  │  │  ├─ oudia/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ document.rs
│  │  │  │  ├─ diagram.rs
│  │  │  │  ├─ train.rs
│  │  │  │  ├─ station_slot.rs
│  │  │  │  ├─ eki_jikoku.rs
│  │  │  │  ├─ file_type.rs
│  │  │  │  └─ source_range.rs
│  │  │  ├─ matching/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ route_matcher.rs
│  │  │  │  ├─ station_matcher.rs
│  │  │  │  ├─ candidate.rs
│  │  │  │  └─ score.rs
│  │  │  ├─ timetable/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ service_time.rs
│  │  │  │  ├─ generator.rs
│  │  │  │  ├─ rounding.rs
│  │  │  │  └─ generated_stop.rs
│  │  │  ├─ editing/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ replacement.rs
│  │  │  │  ├─ patch.rs
│  │  │  │  └─ diff_policy.rs
│  │  │  ├─ validation/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ route_validation.rs
│  │  │  │  ├─ timetable_validation.rs
│  │  │  │  └─ save_validation.rs
│  │  │  └─ error.rs
│  │  └─ tests/
│  │     ├─ eki_jikoku.rs
│  │     ├─ route_matching.rs
│  │     ├─ timetable_generation.rs
│  │     └─ patch_generation.rs
│  │
│  ├─ mtr-oudia-application/
│  │  ├─ Cargo.toml
│  │  ├─ src/
│  │  │  ├─ lib.rs
│  │  │  ├─ ports/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ mtr_api_client.rs
│  │  │  │  ├─ listening_port_provider.rs
│  │  │  │  ├─ oudia_repository.rs
│  │  │  │  ├─ settings_repository.rs
│  │  │  │  └─ log_sink.rs
│  │  │  ├─ use_cases/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ detect_mtr_endpoint.rs
│  │  │  │  ├─ fetch_mtr_snapshot.rs
│  │  │  │  ├─ inspect_oudia.rs
│  │  │  │  ├─ find_route_candidates.rs
│  │  │  │  ├─ build_preview.rs
│  │  │  │  └─ save_conversion.rs
│  │  │  ├─ dto/
│  │  │  │  ├─ mod.rs
│  │  │  │  ├─ endpoint.rs
│  │  │  │  ├─ candidate.rs
│  │  │  │  ├─ preview.rs
│  │  │  │  └─ save_result.rs
│  │  │  └─ error.rs
│  │  └─ tests/
│  │     ├─ detect_endpoint.rs
│  │     ├─ build_preview.rs
│  │     └─ save_conversion.rs
│  │
│  └─ mtr-oudia-infrastructure/
│     ├─ Cargo.toml
│     ├─ src/
│     │  ├─ lib.rs
│     │  ├─ http/
│     │  │  ├─ mod.rs
│     │  │  ├─ reqwest_mtr_client.rs
│     │  │  ├─ response.rs
│     │  │  └─ probe.rs
│     │  ├─ endpoint/
│     │  │  ├─ mod.rs
│     │  │  ├─ discovery.rs
│     │  │  └─ cached_endpoint.rs
│     │  ├─ platform/
│     │  │  ├─ mod.rs
│     │  │  ├─ windows/
│     │  │  │  ├─ mod.rs
│     │  │  │  └─ listening_ports.rs
│     │  │  ├─ linux/
│     │  │  │  ├─ mod.rs
│     │  │  │  └─ listening_ports.rs
│     │  │  └─ unsupported.rs
│     │  ├─ filesystem/
│     │  │  ├─ mod.rs
│     │  │  ├─ oudia_repository.rs
│     │  │  ├─ atomic_writer.rs
│     │  │  ├─ encoding.rs
│     │  │  └─ line_ending.rs
│     │  ├─ settings/
│     │  │  ├─ mod.rs
│     │  │  ├─ json_settings.rs
│     │  │  └─ app_directories.rs
│     │  ├─ logging/
│     │  │  ├─ mod.rs
│     │  │  └─ tracing_setup.rs
│     │  └─ error.rs
│     └─ tests/
│        ├─ endpoint_probe.rs
│        ├─ atomic_write.rs
│        └─ mtr_response.rs
│
├─ desktop/
│  ├─ package.json
│  ├─ vite.config.ts
│  ├─ svelte.config.js
│  ├─ tsconfig.json
│  ├─ src/
│  │  ├─ app.html
│  │  ├─ App.svelte
│  │  └─ lib/
│  │     ├─ api/
│  │     │  ├─ tauri.ts
│  │     │  └─ types.ts
│  │     ├─ stores/
│  │     │  ├─ workflow.ts
│  │     │  ├─ endpoint.ts
│  │     │  ├─ mtr.ts
│  │     │  ├─ oudia.ts
│  │     │  └─ preview.ts
│  │     ├─ components/
│  │     │  ├─ ErrorPanel.svelte
│  │     │  ├─ LoadingOverlay.svelte
│  │     │  └─ TimeTable.svelte
│  │     └─ screens/
│  │        ├─ ApiConnectionScreen.svelte
│  │        ├─ RouteSelectionScreen.svelte
│  │        ├─ OudiaFileScreen.svelte
│  │        ├─ TrainSelectionScreen.svelte
│  │        ├─ RouteMatchingScreen.svelte
│  │        ├─ PreviewScreen.svelte
│  │        └─ SaveResultScreen.svelte
│  └─ src-tauri/
│     ├─ Cargo.toml
│     ├─ tauri.conf.json
│     ├─ capabilities/
│     │  └─ default.json
│     └─ src/
│        ├─ main.rs
│        ├─ lib.rs
│        ├─ state.rs
│        ├─ bootstrap.rs
│        ├─ commands/
│        │  ├─ mod.rs
│        │  ├─ detect_endpoint.rs
│        │  ├─ load_mtr.rs
│        │  ├─ inspect_oudia.rs
│        │  ├─ find_candidates.rs
│        │  ├─ preview.rs
│        │  └─ save.rs
│        └─ dto/
│           ├─ mod.rs
│           ├─ request.rs
│           ├─ response.rs
│           └─ error.rs
│
├─ fixtures/
│  ├─ mtr/
│  └─ oudia/
│
└─ docs/
   ├─ architecture.md
   ├─ mtr-api.md
   ├─ oudia-format.md
   └─ compatibility.md
```

---

## 8. 推奨技術構成

```text
コア言語: Rust
GUIランタイム: Tauri 2
フロントエンド: TypeScript + Svelte + Vite
GUIデザイントークン: @digital-go-jp/design-tokens 2.0.1
非同期ランタイム: Tokio
HTTP: reqwest
JSON: serde / serde_json
エラー: thiserror
ログ: tracing / tracing-subscriber
設定: JSON
テスト: cargo test
初期配布: Windows x64インストーラー
後続候補: Linux x86_64 AppImage
```

### 8.1 CLIを作らない理由

本ソフトはGUI利用を前提とし、処理の再利用はCLIではなくRust crateの分離で実現する。

```text
Rust Domain / Application
  ├─ Tauri GUIから直接呼び出す
  └─ テストから直接呼び出す
```

子プロセス、標準入出力JSON、sidecarは通常処理に使用しない。

### 8.2 Cargo Workspace

ルート `Cargo.toml` の概念構成は次とする。

```toml
[workspace]
resolver = "2"
members = [
    "crates/mtr-oudia-domain",
    "crates/mtr-oudia-application",
    "crates/mtr-oudia-infrastructure",
    "desktop/src-tauri",
]

[workspace.package]
edition = "2024"
version = "0.1.0"

[workspace.dependencies]
thiserror = "2"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
```

バージョン番号は実装開始時に最新の互換版へ固定し、ロックファイルをコミットする。

### 8.3 GUIデザイントークン

GUIの色、タイポグラフィ、余白、角丸、境界、フォーカス表示、状態色およびエレベーションは、デジタル庁デザインシステムのnpmパッケージ [`@digital-go-jp/design-tokens`](https://www.npmjs.com/package/@digital-go-jp/design-tokens) を基準に実装する。

初期採用バージョンは `2.0.1` とし、`desktop/package.json` とロックファイルで固定する。

```bash
npm install @digital-go-jp/design-tokens@2.0.1
```

フロントエンドのエントリーポイントで、パッケージが提供する完全版の `tokens.css` を読み込む。GUIコンポーネントのCSSでは、原則としてパッケージのCSSカスタムプロパティを参照し、同じ値をアプリ側へ重複定義しない。

```ts
import "@digital-go-jp/design-tokens/dist/tokens.css";
```

本パッケージはデザイントークンを提供するものであり、GUIフレームワークや完成済みコンポーネントライブラリを置き換えるものではない。Svelteによる画面・コンポーネント実装は維持し、コンポーネントの構造、状態、キーボード操作およびアクセシビリティ要件は `DESIGN.md` に従う。

パッケージ更新時は、次を確認してからバージョンを変更する。

- CSSカスタムプロパティ名の変更
- DADSトークンとの対応差分
- 色コントラスト
- フォーカス表示
- 文字拡大時のレイアウト
- WindowsおよびLinuxのWebView上での表示

---

## 9. Domain設計

### 9.1 Domainに置く処理

- MTR正規化モデル
- OuDia構造モデル
- `EkiJikoku` セル解析
- 使用駅スロット抽出
- 経路候補照合
- `10:00:00` 起点の時刻計算
- ミリ秒累積
- 出力秒への丸め
- 置換パッチ計画
- 変更許可範囲検証

### 9.2 Domain公開API例

```rust
pub fn generate_timetable(
    route: &MtrRoute,
    template: &OudiaRouteTemplate,
) -> Result<GeneratedTimetable, DomainError>;

pub fn find_route_candidates(
    route: &MtrRoute,
    document: &OudiaDocument,
) -> Result<Vec<RouteCandidate>, DomainError>;

pub fn build_patch(
    source: &OudiaSource,
    template: &OudiaRouteTemplate,
    timetable: &GeneratedTimetable,
    operation_policy: OperationPolicy,
) -> Result<OudiaPatch, DomainError>;
```

### 9.3 Domainで禁止する処理

```text
std::fsによるファイルアクセス
reqwestによるHTTP通信
Windows API
Linux /procやNetlink
Tauri State
GUI通知
設定ファイル読書き
```

---

## 10. Application設計

### 10.1 Use Case

```text
DetectMtrEndpoint
FetchMtrSnapshot
InspectOudia
FindRouteCandidates
BuildPreview
SaveConversion
```

### 10.2 Port trait

```rust
#[async_trait::async_trait]
pub trait MtrApiClient: Send + Sync {
    async fn fetch_snapshot(
        &self,
        endpoint: &MtrEndpoint,
        dimension: u32,
    ) -> Result<MtrNetworkSnapshot, ApplicationError>;
}

pub trait ListeningPortProvider: Send + Sync {
    fn listening_tcp_ports(
        &self,
    ) -> Result<Vec<u16>, ApplicationError>;
}

pub trait OudiaRepository: Send + Sync {
    fn read(
        &self,
        location: &DocumentLocation,
    ) -> Result<OudiaSource, ApplicationError>;

    fn write_atomic(
        &self,
        location: &DocumentLocation,
        bytes: &[u8],
    ) -> Result<(), ApplicationError>;
}
```

### 10.3 Use Caseの責務

Applicationは複数のDomain処理とPortを順序付ける。OuDia文法自体や時刻計算式はApplicationに書かない。

---

## 11. Infrastructure設計

### 11.1 HTTP

`reqwest` を使用し、以下を共通設定する。

- 接続タイムアウト
- 応答タイムアウト
- 応答サイズ上限
- JSON Content-Typeに依存しすぎない安全なJSON判定
- localhost優先
- IPv4とIPv6ループバック候補

### 11.2 ファイル

- 元バイト列を保持する
- 文字コードを検出・保持する
- BOMを保持する
- 改行コードを保持する
- 一時ファイルへ書き出す
- flush・sync後に原子的移動する
- 元ファイルを既定で上書きしない

### 11.3 OS別モジュール

```rust
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
mod linux;
```

初期正式ビルドではWindows実装を必須とし、Linux実装は後続フェーズで有効化する。

---

## 12. OS依存処理

### 12.1 Windows

待受TCPポート取得にはWindows IP Helper APIを使用する。

```text
GetExtendedTcpTable
  ↓
TCP LISTEN状態のローカルポートを列挙
```

PowerShellや `netstat` は主方式にしない。Windows APIが利用できない場合の診断用フォールバックとしてのみ検討する。

### 12.2 Linux

後続実装では次のいずれかを使用する。

```text
第一候補: Netlink
簡易実装: /proc/net/tcp と /proc/net/tcp6
```

Linux固有処理は `platform/linux` 内に閉じ込め、DomainとApplicationの変更を不要にする。

### 12.3 未対応OS

未対応OSでは自動待受ポート列挙を利用不可とし、手動URL入力のみを許可する設計とする。ただし初期正式配布自体は行わない。

---

## 13. MTR APIのマルチサーバー動作

### 13.1 クライアントlocalhost API

マルチサーバー接続時、MTRクライアントは空きポートを選んでローカルWebサーバーを起動する。ポートは `8888` 固定とは限らない。

```text
本アプリ
  ↓ HTTP
127.0.0.1:{動的ポート}
  ↓ MTRクライアント
Minecraftパケット
  ↓
接続中のMTRサーバー
  ↓
MTRサーバー内部API
```

### 13.2 成立条件

- Minecraftクライアントが起動している
- クライアントへMTRが導入されている
- MTR対応マルチサーバーへ接続している
- サーバー側MTR Web APIが有効である
- クライアント・サーバー間のMTR API転送が正常である

### 13.3 接続状態の分類

```text
API候補なし
  → Minecraft未起動、MTR未導入、またはローカルWebサーバー未起動

API候補あり・応答タイムアウト
  → サーバー未接続、転送不能、サーバー側API無効など

API応答あり・路線0件
  → 接続成功だが路線なし、またはdimension違い

API応答あり・路線あり
  → 利用可能
```

---

## 14. MTR APIエンドポイント

```text
http://127.0.0.1:{port}/mtr/api/map/stations-and-routes?dimension={dimension}
```

IPv6ループバック候補:

```text
http://[::1]:{port}/mtr/api/map/stations-and-routes?dimension={dimension}
```

手動入力時も、既定ではループバックアドレスのみを許可する。リモートホスト直接接続は正式要件としない。

---

## 15. MTR API応答構造

### 15.1 共通包絡

API応答は次の共通包絡を持つものとして解析する。

```json
{
  "status": 200,
  "text": "OK",
  "currentTime": 1715000000000,
  "data": {
    "stations": [],
    "routes": [],
    "dimensions": []
  }
}
```

### 15.2 必須検証

- `status` が数値
- `data` が存在
- `data.stations` が配列
- `data.routes` が配列
- `data.dimensions` が配列
- 各routeに `id`、`name`、`stations`、`durations` が存在
- 各route stationに `id`、`name`、`dwellTime` が存在

未知フィールドは無視してよいが、不足した必須フィールドを推測しない。

### 15.3 APIの時間データ

本用途で使用するAPIは絶対時刻ではなく、次の相対時間を返す。

```text
route.stations[i].dwellTime
  → 当該ホームの停車時間（ミリ秒）

route.durations[i]
  → stations[i]発車からstations[i + 1]到着までの運転時間（ミリ秒）
```

ライブ出発時刻を返す別APIは、基準運転時分生成には使用しない。

---

## 16. APIポート自動検出

### 16.1 目的

ユーザーが動的なlocalhostポートを調べなくても接続できるようにする。

### 16.2 検出手順

```text
1. 前回成功したURLを最優先で確認
2. OS上のTCP待受ポートを列挙
3. ループバック待受候補を抽出
4. 候補へ制限付き並列HTTPプローブ
5. MTR共通包絡とstations/routes構造を検証
6. 有効候補を一覧化
7. 路線あり候補を優先表示
8. ユーザー選択または一意候補を自動採用
```

### 16.3 行わない探索

- `1..65535` の全ポートHTTP総当たり
- `8888..9000` のみを前提とする探索
- HTTP 200だけを根拠にMTR APIと判定

### 16.4 プローブ制限

初期値案:

```text
接続タイムアウト: 300ms
全体応答タイムアウト: 1500ms
最大同時プローブ数: 16
最大応答サイズ: 8MiB
```

マルチサーバーへの転送には時間がかかる可能性があるため、TCP接続とAPI応答のタイムアウトを分ける。

### 16.5 複数候補

```text
127.0.0.1:49182
  dimension: 0
  路線数: 14

127.0.0.1:51704
  dimension: 0
  路線数: 0
```

候補が複数ある場合は自動確定せず、ユーザーが選択可能とする。

### 16.6 手動入力

自動検出に失敗した場合はベースURLを手動入力できる。

```text
http://127.0.0.1:49182/
```

入力URLは正規化し、パス重複や末尾スラッシュ差を吸収する。

---

## 17. MTRデータ正規化

### 17.1 API上の駅とホーム

トップレベル `data.stations[]` は駅情報であり、`routes[].stations[]` は路線上のホーム列として扱う。

```text
routes[].stations[i].id
  ↓ ID結合
data.stations[].id
  ↓
正式な駅名

routes[].stations[i].name
  ↓
ホーム名・ホーム番号
```

### 17.2 正規化モデル

```rust
pub struct MtrNetworkSnapshot {
    pub base_url: String,
    pub dimension: u32,
    pub api_current_time_millis: i64,
    pub retrieved_at_unix_millis: i64,
    pub routes: Vec<MtrRouteSnapshot>,
}

pub struct MtrRouteSnapshot {
    pub route_id: String,
    pub display_name: String,
    pub stations_signature: Vec<String>,
    pub stops: Vec<MtrStopSnapshot>,
}

pub struct MtrStopSnapshot {
    pub station_id: String,
    pub station_name: String,
    pub platform_name: String,
    pub dwell_millis: i64,
    pub run_millis_to_next: Option<i64>,
}
```

### 17.3 路線識別

一次キー:

```text
route.id
```

復旧・比較用署名:

```text
路線名
＋ 駅ID列
＋ 正規化駅名列
＋ 駅順
```

route IDがワールド再生成後も永久不変であるとは仮定しない。

### 17.4 データ整合性

- `durations.len() == stations.len() - 1` を基本条件とする
- 最終駅の `run_millis_to_next` は `None`
- 負数の時分は不正
- 異常に大きい値は警告対象
- station ID結合に失敗した場合は駅名を推測せず、当該路線をエラー候補とする

---

## 18. MTR名称の解析

MTR名称は次の順で処理する。

```text
園戸|Sonoto||Airport Branch
  ↓ 「||」で名称部と補足部を分離
名称部: 園戸|Sonoto
  ↓ 「|」で言語別名称を分離
第一表示名: 園戸
```

- `|` は言語別名称区切りとして扱う
- `||` 以降は補足情報として分離する
- GUI初期表示は先頭の空でない名称
- 元文字列と全言語名を保持する
- 駅照合では全言語名を候補として利用できる

---

## 19. MTRスナップショット

API接続後の正規化データをメモリに保持する。

任意でJSON保存を可能にする。

```text
mtr-snapshot-20260804-024500.json
```

用途:

- Minecraft終了後の作業継続
- バグ報告
- API変更調査
- テストfixture
- 再現性確保

生API応答の保存はユーザーが明示的に有効化した場合のみとする。

---

## 20. OuDiaファイル対応方針

### 20.1 既存ファイルを使用

新規 `.oud2` をゼロから生成しない。既存ファイル内の駅、列車種別、ダイヤ、基準列車を使用する。

### 20.2 正式対応FileType

正式な読書き対象:

```text
OuDiaSecond.1.16
OuDiaSecond.1.17
```

その他のFileType:

- ヘッダー情報の表示は可能
- 保存処理は原則禁止
- 明示的な互換テストを追加した後に対応対象へ昇格

FileTypeは保存時に元値を保持し、自動変換しない。

### 20.3 Lossless編集の定義

Losslessを次の2段階で定義する。

```text
無変更保存:
入力ファイルと出力ファイルがバイト単位で同一

時刻変更保存:
許可された対象列車の時刻部分以外がバイト単位で同一
```

全ASTを再シリアライズする方式を主方式にしない。元バイト列とSourceRangeを保持し、対象範囲だけをパッチする。

### 20.4 未知項目

- 未知の行は元バイト列を保持する
- 未知のセクションは位置を保持する
- 未知の `EkiJikoku` 形式で時刻位置を安全に特定できない場合は保存を中止する
- 未知形式を推測して書き換えない

---

## 21. OuDia内部モデル

```rust
pub struct OudiaSource {
    pub bytes: Vec<u8>,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub document: OudiaDocument,
}

pub struct SourceRange {
    pub start: usize,
    pub end: usize,
}

pub struct OudiaProperty {
    pub key: String,
    pub value: String,
    pub whole_line_range: SourceRange,
    pub value_range: SourceRange,
}
```

構造解析モデルは検索・検証に使用し、保存はSourceRangeパッチを基本とする。

---

## 22. サンプルファイルから確認した前提

`Shinkyu.oud2`:

```text
FileType=OuDiaSecond.1.16
Rosenmei=進急本線
KijunDiaIndex=0
```

基準運転時分取得元は0番目のダイヤ。

```text
DiaName=基準運転時分
```

同名駅が複数登録されている。

```text
園戸:
  スロット3
  スロット9

さつたば:
  スロット5
  スロット13
  スロット15

西四稜角:
  スロット11
  スロット14
```

列車種別:

```text
0 普通
1 快速
2 急行
3 進急特快
```

したがって、駅名単体ではなく駅スロット列と前後関係で経路を決定する。

---

## 23. 基準ダイヤ特定

第一優先:

```text
KijunDiaIndex
```

処理:

```text
KijunDiaIndex=n
  ↓
n番目のDiaを取得
  ↓
そのKudari/Nobori内のRessyaを経路テンプレート候補にする
```

異常時:

- プロパティが存在しない
- 数値ではない
- ダイヤ数の範囲外

この場合はGUIで対象ダイヤを選択させる。ダイヤ名が「基準運転時分」であるかは補助情報とする。

---

## 24. 経路テンプレート

```rust
pub struct OudiaRouteTemplate {
    pub diagram_index: usize,
    pub direction: OudiaDirection,
    pub train_index: usize,
    pub train_type_index: usize,
    pub active_station_slots: Vec<usize>,
    pub stop_pattern: Vec<OudiaStopState>,
    pub source_train_range: SourceRange,
    pub eki_jikoku_value_range: SourceRange,
}
```

### 24.1 ActiveStationSlots

`EkiJikoku` を全駅スロット分に分解し、列車が使用するセルを抽出する。

```text
全駅スロット:
0,1,2,3,4,5,6,7,8,9,10

列車が使用:
0,1,2,3,9,10
```

この並びを経路の主要識別情報として使用する。

### 24.2 テンプレートから保持するもの

- `Kudari` / `Nobori`
- 列車種別
- 使用駅スロット
- 空欄駅スロット
- 始発・終着位置
- 停車・通過状態
- 番線
- セル形式
- 列車番号・列車名・備考
- 未知の列車属性

---

## 25. MTR路線とOuDia経路の照合

### 25.1 基本方針

駅単体ではなく、MTR駅列とOuDiaテンプレートの使用駅スロット列を比較する。

```text
MTR:
園戸 → さつたば → 小麻森林公園 → 籠岡

OuDia候補:
園戸[3] → さつたば[5] → 小麻森林公園[6] → 籠岡[8]
```

### 25.2 駅名正規化

- 前後空白除去
- Unicode正規化
- 全角・半角英数字の統一
- 連続空白の整理
- `||` より前の名称部を使用
- `|` で分けた各言語名を候補化
- 保存済み別名辞書を適用

原則として無条件の部分一致は使用しない。

### 25.3 一致優先順位

1. 駅名列が完全一致
2. 別言語名を含めて完全一致
3. 別名適用後に完全一致
4. 接続表現用の連続同名駅を整理すると一致
5. 始発・終着・中間駅の高割合一致
6. 手動対応で全駅を確定

### 25.4 自動確定条件

- 最高スコア候補が一意
- 全MTR駅が対応
- 順序が一致
- 方向がテンプレートから確定
- 列車種別が選択済み
- 同名駅の曖昧性が経路全体で解消

満たさない場合はユーザー確認を要求する。

---

## 26. デルタ線・分岐線

駅スロット番号の増減だけで方向や経路を判断しない。

```text
MTR駅名列
  ↓
OuDiaテンプレート候補と比較
  ↓
使用駅スロット列を確定
  ↓
テンプレート所属方向を採用
```

同名駅もスロット単位で別物として扱う。

```text
園戸 [スロット3 / 前駅A・次駅B]
園戸 [スロット9 / 前駅C・次駅D]
```

補助ラベルは前後駅から生成する。

---

## 27. 方向判定

```text
Kudari内のRessya
  → 下り

Nobori内のRessya
  → 上り
```

駅スロット番号の増減は矛盾検出の補助にのみ使用する。

---

## 28. 列車種別と置換対象

### 28.1 列車種別

MTR路線とOuDia列車種別の自動対応は必須としない。ユーザーがOuDia側の列車種別を選択する。

前回選択は路線対応設定へ保存する。

### 28.2 置換対象列車

```text
基準ダイヤ
＋ 方向
＋ 列車種別
＋ 使用駅スロット列
＋ 停車・通過パターン
```

一致する既存列車1本を置換対象とする。同一条件が複数ある場合はユーザーが選択する。

### 28.3 変更対象

変更するもの:

- 着時刻
- 発時刻
- Operation削除を選択した場合の対象列車Operation情報

原則保持するもの:

- 経路
- 空欄駅スロット
- 停車・通過コード
- 番線
- 始発・終着セル形式
- 列車種別
- 方向
- 列車番号
- 列車名
- 備考
- 未知属性

---

## 29. 発着時刻生成

### 29.1 基準始発時刻

```text
10:00:00 固定
```

GUIには読取専用で表示する。設定ファイルやコマンド引数で変更する機能も正式版には設けない。

### 29.2 計算式

```text
始発駅発時刻
  = 10:00:00

次駅到着
  = 現駅発時刻
  + route.durations[i]

中間駅発時刻
  = 中間駅到着
  + route.stations[i].dwellTime
```

### 29.3 始発・中間・終着

```text
始発駅:
  発時刻のみ

中間駅:
  着時刻と発時刻

終着駅:
  着時刻のみ
```

始発駅の `dwellTime` と終着駅の `dwellTime` は時刻生成へ加算しない。

### 29.4 ミリ秒累積

各区間を個別に秒へ丸めてから加算しない。

```text
10:00:00.000
  + 区間1のミリ秒
  + 駅2の停車ミリ秒
  + 区間2のミリ秒
  ...
```

内部時刻はサービス日開始からのミリ秒で保持する。

```rust
pub struct ServiceTimeMillis(pub i64);
```

### 29.5 出力丸め

OuDia時刻文字列を生成する直前に、累積時刻を秒へ丸める。

非負値の丸め規則:

```text
rounded_seconds = (milliseconds + 500) / 1000
```

同じ絶対時刻を複数回変換しても同じ結果になる決定的な規則とする。

### 29.6 時刻逆行

丸め後に以下を検証する。

- 到着時刻が前駅発時刻より前にならない
- 発時刻が同駅到着時刻より前にならない
- 次駅到着が現駅発時刻より前にならない

---

## 30. 24時超の扱い

内部モデルは24時を超える値を保持できる。

```text
ServiceTimeMillis >= 24時間
```

ただしOuDiaSecond 1.16/1.17の保存表現は、公式アプリで生成したfixtureにより確定する。

正式リリース前の必須検証:

1. `23:59:50 → 00:00:10`
2. `00:00:00` 到着
3. 24時間を超える列車
4. ダイヤグラム起点時刻をまたぐ列車

表現が確定していない段階では、24時以上の時刻を含む場合に保存を禁止し、検証不足エラーを表示する。正式版完了条件には24時超の互換試験合格を含める。

---

## 31. EkiJikoku処理

### 31.1 基本形式

基本セルは次の要素を持つものとして解析する。

```text
駅扱い;着時刻/発時刻$番線
```

実際の省略形・追加属性を考慮し、元文字列とSourceRangeを保持する。

### 31.2 セルモデル

```rust
pub struct EkiJikokuCell {
    pub raw: String,
    pub handling_code: Option<u8>,
    pub arrival: Option<OudiaTime>,
    pub departure: Option<OudiaTime>,
    pub track_index: Option<u32>,
    pub unknown_parts: Vec<String>,
    pub source_range: SourceRange,
}
```

### 31.3 更新

- MTR駅に対応する駅スロットだけ更新
- 空欄セルを埋めない
- 使用駅スロットを増減しない
- 停車・通過コードを変更しない
- 番線を変更しない
- 時刻部分のSourceRangeだけを置換

### 31.4 未知形式

次の場合は保存を中止する。

- セル境界を安全に分解できない
- 着時刻・発時刻の位置を一意に特定できない
- 既知形式ではないが書換えが必要
- 再解析結果が元のセル構造と一致しない

### 31.5 再生成後の保証

- 駅スロット数不変
- 空欄位置不変
- 使用駅スロット列不変
- 番線不変
- 停車・通過状態不変
- 更新時刻が再解析可能
- 許可範囲外のバイト列不変

---

## 32. Operation情報

対象列車に `Operation*` が存在する場合、次の2方式を正式に提供する。

### 32.1 保持

```text
Operation文字列を元のまま保持
```

- 生文字列を変更しない
- 保存前に明示警告を表示
- ファイル保持と再解析は保証する
- 時刻変更後の運用上の意味整合性は保証しない

### 32.2 削除

```text
対象列車に属するOperation情報だけを削除
```

- 他列車のOperation情報を変更しない
- 削除範囲をSourceRangeで特定する
- 保存後に対象範囲外不変を検証する

### 32.3 既定動作

Operation情報が存在しない場合は通常処理する。

存在する場合は選択を必須とし、暗黙に処理しない。初期選択は「保持」とするが、保存前確認を必須とする。

完全なOperation再計算は対象外とする。

---

## 33. GUIフロー

### Step 1: MTR接続

```text
MTR API

状態: 未接続
dimension: [0]

[自動検出]
API URL: [                                ]
[接続]
```

自動検出中:

```text
localhostの待受ポートを確認中...
候補を検証中: 8 / 42
```

接続成功後:

```text
接続先: 127.0.0.1:49182
取得路線数: 14
取得API時刻: 2026-08-04 02:45:00
```

### Step 2: dimension選択

`data.dimensions` が複数ある場合に表示する。

```text
dimension:
[ overworld (0) ▼ ]
```

1件だけの場合は自動選択する。

### Step 3: MTR路線選択

```text
路線:
[ 園戸線 籠岡方面 ▼ ]

駅数: 4
総運転時分: 4:15.230
総中間停車時分: 1:02.000
```

簡易表:

```text
園戸
  ホーム: 1番線
  ↓ 運転 1:31.240

さつたば
  停車 0:25.000
  ↓ 運転 1:02.490
```

### Step 4: OuDiaファイル選択

```text
OuDiaファイル:
[ C:\...\Shinkyu.oud2 ] [選択]
```

読込後:

```text
路線名: 進急本線
FileType: OuDiaSecond.1.16
基準ダイヤ: 基準運転時分
駅数: 16
Lossless解析: 成功
```

### Step 5: 列車種別・経路選択

```text
OuDia列車種別:
[ 普通 ▼ ]

経路候補:
● 下り / 本線経由
  園戸[3] → さつたば[5]
  → 小麻森林公園[6] → 籠岡[8]

○ 別経路
  園戸[9] → ...
```

### Step 6: 駅対応確認

```text
MTR駅             MTRホーム     OuDia駅
園戸              1番線         園戸 [3]
さつたば          2番線         さつたば [5]
小麻森林公園      1番線         小麻森林公園 [6]
籠岡              3番線         籠岡 [8]
```

### Step 7: 時刻プレビュー

```text
基準始発時刻: 10:00:00（固定）

駅                 既存着     既存発     新着       新発
園戸                                      -          10:00:00
さつたば           11:01:28   11:01:38   10:01:31   10:01:56
小麻森林公園       ...        ...        10:02:58   10:03:15
籠岡               ...        -          10:04:57   -
```

表示項目:

- 元ミリ秒
- 丸め後時刻
- 区間運転時分
- 中間停車時分
- 警告

### Step 8: Operation選択

Operationが存在する場合のみ表示する。

```text
Operation情報が存在します。

● 元のまま保持する
  意味上の整合性は保証されません

○ 対象列車から削除する
```

### Step 9: 保存

```text
出力先:
C:\...\Shinkyu_MTR基準時分更新.oud2

変更対象:
EkiJikoku時刻部分のみ

[保存]
```

---

## 34. Tauriコマンド

TauriコマンドはApplication Use Caseを薄く呼び出す。

```rust
#[tauri::command]
pub async fn detect_mtr_endpoints(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<EndpointDto>, ErrorDto> {
    state
        .detect_mtr_endpoint
        .execute()
        .await
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}
```

公開コマンド案:

```text
detect_mtr_endpoints
fetch_mtr_snapshot
inspect_oudia
find_route_candidates
build_preview
save_conversion
```

TypeScript側でファイル内容を読み込まず、ファイル選択後のパスをRust側へ渡す。

---

## 35. 設定保存

### 35.1 保存場所

OS標準ディレクトリを利用する。

Windows:

```text
%APPDATA%\MtrOudiaConverter\
```

Linux後続版:

```text
$XDG_CONFIG_HOME/mtr-oudia/
$XDG_DATA_HOME/mtr-oudia/
$XDG_STATE_HOME/mtr-oudia/
```

環境変数が未設定の場合は標準的なホーム配下へフォールバックする。

### 35.2 ファイル

```text
settings.json
station-aliases.json
route-mappings.json
logs/
snapshots/
```

### 35.3 API設定

```json
{
  "lastBaseUrl": "http://127.0.0.1:49182/",
  "lastDimension": 0,
  "autoDetectPorts": true
}
```

ポートはセッションごとに変わる可能性があるため、前回URLは高速化候補であり、永続的な正解とはみなさない。

### 35.4 駅別名

```json
{
  "小麻森林公園前": "小麻森林公園",
  "中央駅": "中央"
}
```

### 35.5 路線対応

```json
{
  "mtrRouteId": "route-id",
  "mtrRouteSignature": "route-signature",
  "oudiaRouteSignature": "oudia-route-signature",
  "diagramIndex": 0,
  "direction": "Kudari",
  "trainTypeIndex": 0,
  "stationSlots": [3, 5, 6, 8],
  "templateTrainIndex": 2
}
```

---

## 36. 安全な保存

### 36.1 上書き禁止

元ファイルは既定で上書きしない。

```text
Shinkyu.oud2
  ↓
Shinkyu_MTR基準時分更新.oud2
```

### 36.2 保存手順

```text
1. 元ファイルの現在バイト列を再読込
2. 読込時ハッシュと比較し、外部変更を検出
3. Domainでパッチ計画を生成
4. 許可SourceRangeだけを置換
5. 変更前後のバイト差分を検証
6. 一時ファイルへ出力
7. flush・sync
8. 一時ファイルを再解析
9. 構造・時刻・FileType・経路を検証
10. 正常なら指定出力先へ原子的に移動
```

### 36.3 外部変更

プレビュー後に元ファイルが変更されていた場合は保存を中止し、再読込を要求する。

### 36.4 将来の上書きモード

上書き機能を追加する場合は自動バックアップを必須とする。

```text
Shinkyu.oud2.bak
```

---

## 37. 保存前検証

- MTR API接続または有効なスナップショットが存在
- MTR路線を選択済み
- station ID結合が完了
- `durations` 長が正しい
- 時分が負数でない
- `.oud2` のLossless解析に成功
- FileTypeが正式対応対象
- 基準ダイヤが確定
- OuDia列車種別が確定
- 経路テンプレートが確定
- 全MTR駅がOuDia駅スロットへ対応
- 方向が確定
- 対象列車が1本に確定
- `10:00:00` 起点の時刻生成に成功
- 丸め後の時刻が逆行しない
- `EkiJikoku` 全対象セルが既知形式
- 駅スロット数が不変
- Operation方針が確定
- 出力先が元ファイルと異なる
- 24時超時刻の互換仕様が検証済み、または時刻が24時未満

---

## 38. 保存後検証

- `FileType` が元値と一致
- 路線名が一致
- 駅数が一致
- 列車種別数が一致
- ダイヤ数が一致
- `KijunDiaIndex` が一致
- 対象列車が存在
- 方向が一致
- 使用駅スロット列が一致
- 停車・通過状態が一致
- 番線が一致
- 新しい着発時刻がプレビューと一致
- Operation方針が反映
- 無関係な列車がバイト単位で不変
- 許可範囲外のバイト列が不変
- 出力を再解析可能

検証失敗時は正式ファイルとして確定しない。

---

## 39. エラー設計

### 39.1 APIが見つからない

```text
MTR APIを検出できませんでした。

確認項目:
・Minecraftが起動している
・MTR対応マルチサーバーへ接続している
・クライアントとサーバーに対応MTRが導入されている
・サーバー側MTR Web APIが有効である
```

手動URL入力を案内する。

### 39.2 API候補はあるが応答しない

- 接続先サーバーの状態を確認
- 転送応答タイムアウトとして表示
- ポート候補自体は診断情報に残す
- 無限待機しない

### 39.3 API JSONが想定外

- 応答サイズ制限内の生レスポンスを診断保存可能
- 不足フィールドを表示
- 未知フィールドはエラーにしない
- `.oud2` を変更しない

### 39.4 経路候補なし

- MTR駅列を表示
- OuDia経路候補を表示
- 駅別名設定を提案
- 手動駅対応画面を開く

### 39.5 経路候補複数

自動確定せず候補比較画面を表示する。

### 39.6 テンプレート列車なし

処理を中止する。新規基準列車生成は対象外。

### 39.7 未知EkiJikoku形式

- 問題セルと元文字列を表示
- 保存を禁止
- 元ファイルを変更しない

### 39.8 保存後の再解析失敗

- 一時ファイルを正式名へ移動しない
- 元ファイルを変更しない
- エラーログを保存
- プレビュー状態を保持

---

## 40. ログ

記録項目:

```text
アプリバージョン
Rust/Tauriバージョン
OS・アーキテクチャ
API検出方法
MTR API URL
ポート候補数
dimension
API取得日時
API currentTime
API路線数
選択MTR路線ID・署名
OuDia FileType
OuDia路線名
入力ファイルハッシュ
KijunDiaIndex
選択方向
選択列車種別
選択駅スロット列
対象列車インデックス
Operation方針
出力先
変更範囲
再解析結果
差分検証結果
エラー
```

記録しないもの:

- Minecraft認証情報
- セッショントークン
- プレイヤー個人情報
- 不要なサーバーアドレス

---

## 41. テスト設計

### 41.1 Domain単体テスト

- `EkiJikoku` 基本セル
- 空欄セル
- 発時刻のみ
- 着時刻のみ
- 着発時刻
- 番線付き
- 通過
- 始発
- 終着
- 未知形式の安全失敗
- 使用駅スロット抽出
- 同名駅照合
- デルタ線候補
- 分岐線候補
- 上下方向
- `10:00:00` 固定
- ミリ秒累積
- 500ms丸め境界
- 24時境界
- Operation保持パッチ
- Operation削除パッチ
- 許可範囲外差分拒否

### 41.2 Applicationテスト

Fake Portを使用する。

- 前回URL成功
- 前回URL失敗後に自動検出
- 候補1件
- 候補複数
- 候補なし
- API取得後プレビュー
- 保存前外部変更検出
- 再解析失敗

### 41.3 Infrastructure APIテスト

fixture応答:

- 正常な共通包絡
- `status != 200`
- `data == null`
- routes 0件
- durations不一致
- station ID不明
- HTML応答
- 巨大応答
- タイムアウト
- IPv4ループバック
- IPv6ループバック

### 41.4 OS別ポート列挙テスト

Windows:

- LISTENポート取得
- IPv4
- IPv6
- 重複除去
- 権限不足時の診断

Linux後続版:

- `/proc/net/tcp`
- `/proc/net/tcp6`
- Netlink実装を採用した場合のfixture

### 41.5 OuDia fixture

```text
fixtures/oudia/
├─ filetype-1.16/
│  ├─ simple.oud2
│  ├─ operations.oud2
│  ├─ branch.oud2
│  └─ overnight.oud2
├─ filetype-1.17/
│  ├─ simple.oud2
│  └─ unknown-fields.oud2
└─ invalid/
   ├─ unknown-eki-jikoku.oud2
   └─ broken-section.oud2
```

### 41.6 Losslessテスト

- 無変更時にSHA-256一致
- 改行コード維持
- BOM維持
- 未知行維持
- 対象時刻外の全バイト一致
- Operation削除時は許可範囲だけ差分
- 保存後再解析成功

### 41.7 Tauriテスト

- 各commandのDTO変換
- ErrorDto変換
- GUI状態遷移
- 保存ボタン有効条件
- Operation警告表示
- 10時固定表示

---

## 42. 実装フェーズとIssue分割

正式版の要求範囲は維持しつつ、実装は以下のIssue単位で進める。各Issueは、依存Issueが完了し、受け入れ条件を満たした時点で完了とする。

### P01: Workspaceと品質ゲート

**依存:** なし

**成果物:**

- Cargo Workspace
- `domain`、`application`、`infrastructure`、`desktop/src-tauri`
- Rust toolchain固定
- formatter、clippy、testの実行手順
- fixtureディレクトリ
- 最小CI

**受け入れ条件:**

- 空実装または最小実装で標準検証コマンドが成功する
- 禁止依存方向がCargo依存関係上も存在しない
- Desktopを除くcrateがTauriへ依存していない

### P02: Domain基礎型

**依存:** P01

**成果物:**

- `ServiceTimeMillis`
- MTR正規化モデル
- OuDia駅スロット識別子
- Domainエラー
- 基本的な値オブジェクト

**受け入れ条件:**

- 不正な負数時間、範囲外インデックス等を型またはコンストラクタで拒否できる
- DomainテストがOS、ネットワーク、ファイルへ依存せず実行できる

### P03: OuDia Lossless読込

**依存:** P01、P02

**成果物:**

- バイト列、BOM、文字コード、改行コードの保持
- セクション解析
- `SourceRange`
- `KijunDiaIndex`、基準ダイヤ、`Ressya`、`EkiJikoku`の読込
- 未知行の保持

**受け入れ条件:**

- 対応fixtureを読み込み、無変更で元バイト列を再現できる
- 未知行を削除・並べ替えしない
- 未対応FileTypeまたは未知セル形式は安全なエラーになる

### P04: 時刻生成Domain

**依存:** P02

**成果物:**

- `10:00:00` 固定の基準時刻
- ミリ秒累積
- 始発・中間・終着の発着生成
- 出力時の秒丸め
- 時刻逆行検証

**受け入れ条件:**

- 始発駅発が常に `10:00:00`
- 各駅到着・発車が設計式どおりである
- 中間計算では丸めを行わない
- 境界値と日付またぎのテストがある

### P05: MTR API手動接続

**依存:** P01、P02

**成果物:**

- HTTP Port traitとreqwest実装
- 共通包絡解析
- station/route ID結合
- dimension、路線、dwellTime、durationsの正規化
- メモリ上のsnapshot

**受け入れ条件:**

- 保存済みJSON fixtureから正規化結果を再現できる
- `routes[].stations[].name`を駅名として誤使用しない
- 配列長不整合や必須項目欠落を明確なエラーにする
- ライブ絶対時刻を基準時刻生成へ使用しない

### P06: 経路テンプレートと照合

**依存:** P03、P05

**成果物:**

- 基準列車からActiveStationSlots抽出
- 多言語名解析と駅名正規化
- 同名駅、方向、デルタ線、分岐線の候補生成
- 自動確定と手動選択用候補

**受け入れ条件:**

- 駅名だけでOuDia駅スロットを直接確定しない
- 一意性が不足する場合は自動確定せず候補を返す
- Kudari/Noboriの逆順候補を区別できる
- 対応なし、単一候補、複数候補のテストがある

### P07: Windows API自動検出

**依存:** P05

**成果物:**

- `GetExtendedTcpTable`によるLISTENポート列挙
- 前回成功URLの優先確認
- 同時実行数とタイムアウトを制限したAPIプローブ
- 複数候補の返却
- 手動URLフォールバック

**受け入れ条件:**

- 1〜65535の全ポート総当たりを行わない
- HTTP 200だけでMTR APIと判定せずJSON構造を検証する
- 候補なし、応答不能、複数候補を区別する
- Windows固有コードが `cfg(target_os = "windows")` 内にある

### P08: SourceRangeパッチと安全保存

**依存:** P03、P04、P06

**成果物:**

- `EkiJikoku`更新計画
- SourceRangeパッチ
- Operation保持・削除
- 一時ファイルへの書込み
- 保存後再解析
- 許可範囲外のバイト比較

**受け入れ条件:**

- 無変更時にSHA-256が一致する
- 時刻変更時は許可範囲外がバイト一致する
- Operation削除時も定義済み範囲外を変更しない
- 再解析または差分検証失敗時は出力成功として扱わない
- 元ファイルを既定で上書きしない

### P09: Applicationユースケース統合

**依存:** P05、P06、P07、P08

**成果物:**

- API検出
- snapshot取得
- OuDia検査
- 候補検索
- プレビュー生成
- 保存
- Portのmockを使ったApplicationテスト

**受け入れ条件:**

- 各ユースケースがInfrastructure具象型を参照しない
- 失敗がGUI表示可能な業務エラーへ分類される
- 保存ユースケースが検証を迂回できない

### P10: Tauri/Svelte GUI

**依存:** P09

**成果物:**

- `@digital-go-jp/design-tokens` 2.0.1の導入
- Step 1〜9の画面
- Tauri commandsとDTO変換
- ファイルダイアログ
- 候補選択、プレビュー、Operation選択、保存結果
- DADSに沿ったフォーカス、エラー、警告表示

**受け入れ条件:**

- GUI内に時刻計算やOuDia構文解析を複製しない
- キーボードだけで主要フローを完了できる
- 処理中、候補複数、警告、保存失敗をテキストでも識別できる
- 基準時刻が固定値として表示され、編集欄になっていない
- `npm run check` と `npm run build` が成功する

### P11: 互換性実証

**依存:** P08、P10

**成果物:**

- OuDiaSecond 1.16/1.17 fixture
- 全正式対応 `EkiJikoku` 形式
- 24時超表現
- `Shinkyu.oud2` 実データ試験
- OuDiaSecond本体での往復試験記録

**受け入れ条件:**

- 第44章の未実証事項について、対応・非対応・保留が記録されている
- 推測だけで正式対応形式を追加していない
- 対応fixtureで保存後にOuDiaSecondが開ける

### P12: Windows正式配布

**依存:** P11

**成果物:**

- Windows x64リリースビルド
- インストーラー
- クリーン環境試験
- 設定移行方針
- 操作マニュアル

**受け入れ条件:**

- Windows 10/11 x64の対象環境で起動・API検出・保存が完了する
- アプリ未導入環境からインストールできる
- 既知の制約と未対応形式が利用者向けに明記されている

### P13: Linux後続対応

**依存:** P12

**成果物:**

- Linux待受ポート列挙
- XDG設定ディレクトリ
- WebKitGTK互換試験
- AppImage等の配布物

**受け入れ条件:**

- Domain/Applicationの業務ロジックを変更せず追加できる
- 対応ディストリビューションと必要ライブラリを明記する
- Windows実装を条件分岐で汚染しない

### 42.1 並行実装してよい範囲

P01とP02の公開境界を確定した後、次は並行実装してよい。

```text
P03 OuDia読込 ─┐
P04 時刻生成   ├─ 並行可能
P05 MTR API    ┘
```

P06はP03とP05、P08はP03・P04・P06の結果を使うため、先行実装の公開型を仮定して進めない。複数エージェントで作業する場合も、公開traitとDTOを先に統合してから分岐する。

### 42.2 Issue作成テンプレート

```markdown
## 目的
設計書の対象章と実現するユーザー価値を書く。

## 対象範囲
変更するcrate、モジュール、公開APIを書く。

## 対象外
このIssueでは実装しない事項を書く。

## 受け入れ条件
- [ ] 正常系
- [ ] 異常系
- [ ] 依存方向
- [ ] Losslessまたは安全停止条件

## テスト
追加するfixture、単体テスト、統合テストを書く。

## 検証コマンド
実行するコマンドを書く。
```

---

## 43. 仕様確定済み事項

- 独立したデスクトップGUIソフト
- Rust Core + Tauri 2 + TypeScript + Svelte
- GUIデザイントークンは `@digital-go-jp/design-tokens` 2.0.1
- CLIなし
- Domain / Application / Infrastructure / Desktop分離
- 初期正式対応はWindows x64
- Linuxは後続対応候補
- localhost APIポートは動的
- OSの待受ポートを列挙してAPIを検証
- マルチサーバー接続時はクライアントAPIがサーバーへ転送
- API応答は共通包絡内の `data`
- 駅名はトップレベルstationsから取得
- route stationsのnameはホーム名として扱う
- `dwellTime` と `durations` はミリ秒の相対時間
- ライブ絶対時刻APIは使用しない
- 始発発時刻は `10:00:00` 固定
- ミリ秒を累積し、出力時だけ秒へ丸める
- 既存基準列車を経路テンプレートにする
- OuDia駅は駅名ではなく駅スロットで扱う
- 方向は `Kudari` / `Nobori` から取得
- `OuDiaSecond.1.16` と `1.17` を正式対象とする
- 保存はSourceRangeパッチ方式
- Operation保持・削除を選択可能
- 元ファイルは既定で上書きしない
- 保存後に再解析と差分範囲検証を行う

---

## 44. リリース前に実証が必要な事項

以下は設計方針ではなく、実装fixtureと公式OuDiaSecondでの往復試験により確定させる。

1. `OuDiaSecond.1.16` の全対象 `EkiJikoku` セル形式
2. `OuDiaSecond.1.17` の全対象 `EkiJikoku` セル形式
3. 24時超時刻の保存表現
4. 日付またぎ列車の表示・再保存
5. Operation削除対象範囲
6. 未知付加属性を含む実ファイルのバイト保持
7. MTR 4.x内のAPI JSON差分fixture
8. Windows上の複数MTRクライアント・複数候補時の選択挙動

実証できない形式は、推測して対応せず安全に保存を中止する。

---

## 45. 正式版完了条件

1. Windows 10/11 x64で動作する
2. マルチサーバー接続中の動的localhostポートを自動検出できる
3. APIの共通包絡、駅、ホーム、運転時分、停車時分を正しく取得できる
4. `10:00:00` 固定で時刻を生成できる
5. ミリ秒累積と出力丸めがfixtureどおりである
6. 同名駅・デルタ線・分岐線を経路テンプレートで処理できる
7. OuDiaSecond 1.16・1.17の正式対象形式を安全に解析できる
8. 対象列車の時刻だけを変更できる
9. Operation保持・削除を選択できる
10. 無変更保存がバイト同一である
11. 時刻変更時、許可範囲外がバイト同一である
12. 保存後再解析と構造検証に成功する
13. 24時超の公式アプリ往復試験に合格する
14. 未知形式では元ファイルを変更せず安全に中止する
15. GUIが処理中・警告・エラーを明確に表示する

---


## 45.1 Codexへのリリース判定依頼

Codexへ「完成したか」を確認させる場合は、コード量や画面の存在ではなく、次を証拠付きで報告させる。

- 第45章の各完了条件に対応するテスト名または実機試験記録
- 標準検証コマンドの結果
- 未実証事項と、正式対応から除外した形式
- 変更可能SourceRangeと実際の差分結果
- Windows実機で検証したAPI検出経路
- GUIのキーボード操作とエラー表示の確認結果

「実装済みだが未検証」は完了として扱わない。

## 46. 設計の核心

MTR側から取得するのは、絶対的な時刻表ではなく相対的な路線時分である。

```text
駅A
  ↓ 運転時分
駅B
  停車時分
  ↓ 運転時分
駅C
```

時刻は本ソフト内で次のように生成する。

```text
駅A 発 10:00:00
  + MTR駅間運転時分
駅B 着
  + MTR中間停車時分
駅B 発
  + MTR駅間運転時分
駅C 着
```

OuDia側では駅名を1駅ずつ直接割り当てず、既存基準列車の経路全体をテンプレートとして使う。

```text
MTRの駅名・駅ID列
  ↕
OuDia基準列車の使用駅スロット列
```

そして、経路、方向、番線、停車形式、空欄駅スロット、未知属性を保持したまま、着発時刻だけをMTR由来の値へ更新する。

この処理をOS非依存のDomainとして実装し、HTTP、ファイル、待受ポート列挙などの依存処理をInfrastructureへ分離する。これにより、Windows版を先に完成させても、OuDia解析・経路照合・時刻計算を書き直さずにLinux対応を追加できる。
