# SILIQ Creative Generator — 設計パッケージ v0.2

作成日：2026-10-10。方式：B（独自デザインエディター）。状態：要件・アーキテクチャの設計案。

**人間がデザインを決め、ローカルの決定論的エンジンが、編集・差し替え・検査・量産を担う。**

完成アプリは完全オフライン。実行時のAI、LLM、機械学習推論、外部API、クラウド、認証、テレメトリーを採用しない。Claude Codeは開発時だけ使用する。

## 読む順序

1. `docs/01_REQUIREMENTS.md`：目的、対象、操作、機能要件、リリース境界。
2. `docs/02_ARCHITECTURE.md`：描画・編集・保存・バッチの構造と処理順。
3. `docs/03_SECURITY_AND_OFFLINE.md`：通信、権限、未信頼ファイル、配布の境界。
4. `docs/04_ACCEPTANCE_AND_PHASES.md`：性能目標、試験、開発ゲート。
5. `docs/05_DATA_MODEL_AND_CONTRACTS.md`：文書モデル、IPC、保存形式。
6. `docs/06_DECISIONS_AND_RISKS.md`：採用判断、撤回条件、未確定事項。
7. `docs/SOURCES.md`：技術的な根拠となる一次資料。
8. `CLAUDE.md` と `START_HERE.md`：開発支援への引き継ぎ。

`schemas/design.core.schema.json` と `examples/design.core.sample.json` は **Phase 0の中核契約案** だ。v1全機能の完成スキーマではない。サンプルの画像・ロゴ・フォントは実ファイルを同梱していないため、そのまま描画できる成果物ではない。

`validation/design_pack_checks.json` はこの設計パッケージのJSON・参照関係などの検証結果だ。完成アプリの画質、速度、オフライン動作を検証した結果ではない。

## 以前のスターターとの関係

これは新しい独立した設計パッケージであり、元のスターターZIPを変更していない。今回の要件が優先される。旧案の「local-first」「最終SVGを主経路」「外部AI APIだけ禁止」を、それぞれ「完全オフライン」「独自文書＋Skia主経路」「ローカル推論を含む実行時AI禁止」へ明確化する。

実装済みのリポジトリへ無条件に上書きしない。既存の差分と仕様を比較し、移行計画を作る。実装がない場合は新規ワークスペースの設計書として使用する。

## 設計上の仮置き

最初の検証対象はmacOS／Apple Silicon、16GBメモリー。初期の品質保証は日本語・英語の横組みとsRGBの静止画制作。これらはユーザーが指定した既成事実ではなく、検証範囲を明確にするための提案だ。Windows、縦組み、商業印刷は別ゲートで追加する。

**本パッケージに実行可能なアプリ、Skiaビルド、実際のフォント、完成したデザイン画像は含まれない。**

## このリポジトリの現状（2026-10-10）

- 設計パッケージ v0.2 を配置した。
- **Phase 0 Step A（headlessのRust CPU描画エンジンとCLI）を実装した。** 結果は `reports/G0-step-a.md`。GUI、macOS実機検証（Step B）は未着手で、G0は未判定。
- 計画：`docs/PHASE0_PLAN.md`。依存と採用理由：`docs/DEPENDENCIES.md`。設計判断：`docs/adr/`。

## Build／test（Step A。Linux x86_64で実行確認済み）

Skiaの配布binaryは `scripts/cargo-verified.sh` 経由で取得する。公式URLから取得し、`third_party/skia-binaries.lock` のSHA-256と照合してからbuildへ渡す（開発時だけ通信する。製品は通信しない）。

```sh
scripts/cargo-verified.sh test --workspace
scripts/cargo-verified.sh clippy --workspace --all-targets -- -D warnings
scripts/cargo-verified.sh build --release -p render-cli
scripts/repeat-render.sh 10 square          # 10回生成してhashを比較
tests/offline/check-deps.sh                 # runtime依存とimportに通信・AI系が無いこと
cargo deny check                            # license / advisory / ban（cargo-denyが必要）

./target/release/siliq-render render \
  --doc fixtures/public-safe/documents/basic.json --variant square \
  --resources fixtures/public-safe/resources \
  --out-dir out --name basic-square.png --report basic-square.report.json
```

| crate | 役割 |
|---|---|
| `crates/design-model` | 文書型、厳格な読み込み、意味検証、variant解決、issue形式 |
| `crates/design-svg` | SVG安全サブセット→独自path |
| `crates/design-render` | 資源読み込み、文字組み、RenderPlan、Skia CPU描画、PNG |
| `crates/render-cli` | `siliq-render`（check／render） |
