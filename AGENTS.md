<!-- headroom:rtk-instructions -->
# RTK (Rust Token Killer) - Token-Optimized Commands

When running shell commands, **always prefix with `rtk`**. This reduces context
usage by 60-90% with zero behavior change. If rtk has no filter for a command,
it passes through unchanged — so it is always safe to use.

## Key Commands
```bash
# Git (59-80% savings)
rtk git status          rtk git diff            rtk git log

# Files & Search (60-75% savings)
rtk ls <path>           rtk read <file>         rtk grep <pattern>
rtk find <pattern>      rtk diff <file>

# Test (90-99% savings) — shows failures only
rtk pytest tests/       rtk cargo test          rtk test <cmd>

# Build & Lint (80-90% savings) — shows errors only
rtk tsc                 rtk lint                rtk cargo build
rtk prettier --check    rtk mypy                rtk ruff check

# Analysis (70-90% savings)
rtk err <cmd>           rtk log <file>          rtk json <file>
rtk summary <cmd>       rtk deps                rtk env

# GitHub (26-87% savings)
rtk gh pr view <n>      rtk gh run list         rtk gh issue list

# Infrastructure (85% savings)
rtk docker ps           rtk kubectl get         rtk docker logs <c>

# Package managers (70-90% savings)
rtk pip list            rtk pnpm install        rtk npm run <script>
```

## Rules
- In command chains, prefix each segment: `rtk git add . && rtk git commit -m "msg"`
- For debugging, use raw command without rtk prefix
- `rtk proxy <cmd>` runs command without filtering but tracks usage
<!-- /headroom:rtk-instructions -->

# 開発方針

本プロジェクトではテスト駆動開発を行います。最初にテスト書く。その上で次のような開発フローとする。

1. パースごとに実装していく
2. テストして確認する
3. テストが通ったら、コミットとプッシュをする
4. 手順1に戻る

実装レビューはこちらが指示するまでやらない。

# ユーザーとの協働

ユーザーを完了報告の受け手ではなく、作業方針を一緒に決める協働者として扱うこと。

- 複数段階の作業を始める前に、理解した目的、最初の行動、結果に影響する仮定を1〜3文で共有する。
- フェーズが変わった時、またはツール・サブエージェントを2〜3回実行した時を目安に、現在地、判明事項、推奨方針、次の行動を1〜3文で共有する。
- 調査によって重要な選択肢、制約、トレードオフが判明した場合は、実装方針を確定する前に共有する。推奨案を明示し、ユーザーが方向性に意見を出せるようにする。
- 回答によって正確性、設計、安全性、費用、またはユーザーから見える挙動が大きく変わる場合だけ、焦点を絞った質問を1つする。それ以外は仮定を明示して進める。
- 破壊的、不可逆、公開、課金、セキュリティ、データ移行、主要API・アーキテクチャ変更の前には、明示的な承認を得る。
- 低リスクで容易に戻せる作業は、定型的な承認待ちで止めない。判断を短く報告して進める。
- サブエージェントへ委譲する前に目的を共有する。分割可能な作業を一度の巨大で不透明な委譲にしない。完了後は、結果、方針への影響、未検証事項を速やかに共有する。
- ブロッカー、テスト失敗、想定外の発見、方針変更は、判明した時点で原因と次の行動を報告する。
- 作業中にユーザーから新しい指示が届いた場合は、次に応答可能な時点で取り込み、続行前に方針を見直す。
- 長時間無言で作業を進めない。ただし、変化のない実況、コマンドの逐次報告、同じ内容の繰り返しは避ける。
- 完了時は、変更内容、検証結果、レビュー結果、残るリスクまたは未完了事項を報告する。