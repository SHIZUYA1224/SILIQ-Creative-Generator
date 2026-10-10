# Phase 0 計画 — 承認待ち

作成日：2026-10-10。状態：**Step A 実装済み（結果は `reports/G0-step-a.md`）。Step B は未着手。** 本書は `START_HERE.md` の初回指示に対する報告として作成した計画であり、計画時点の記述は変更していない。

Step A で計画から変えた点：フォント fixture は案1（Noto Sans JP、OFL）を採用した。macOS の公式 Skia binary に FreeType が無いため、フォント読み込み経路を2種類にした（ADR 0004）。

## 1. 既存リポジトリの確認

- `main` 相当の履歴は `Initial commit` 1件のみ。中身は1行の `README.md` だけだった。
- 旧スターターのコード・schemaは存在しない。差分移行は不要で、設計パッケージ v0.2 をそのまま配置した（`README.md` はパック版で置き換え。旧内容はタイトル1行のみ）。
- パック同梱の `schemas/design.core.schema.json` と `examples/design.core.sample.json` を、このコンテナの Python `jsonschema` 4.26（Draft 2020-12）で再検証した。schema自体は有効、sampleのschemaエラーは0件。意味検証（参照・循環等）はまだ再実行していない。

## 2. 開発環境の確認結果（事実のみ）

この作業は **Claude Codeのクラウドコンテナ（Linux x86_64）** 上で行った。macOS実機ではない。

| 項目 | 結果 |
|---|---|
| OS／CPU | Linux 6.18、x86_64、4コア、メモリー約15GiB |
| ツール | rustc／cargo 1.97.0、Node 22.22、pnpm 10.28、clang 18.1、cmake 3.28、ninja 1.11 |
| 依存取得 | crates.io・npm registry は取得可。`skia.googlesource.com`／`chromium.googlesource.com` はproxyで拒否（403） |
| Skia | scratchpad の使い捨てprobeで `skia-safe 0.153.3`（feature `textlayout`）を **rust-skiaの配布済みbinary** でbuildできた。Skiaのsource buildはこの環境では不可 |
| 出力の反復 | 64×64の矩形1つをCPU raster→PNGで10回出力し、SHA-256が10回一致（`ac5cb6a8…e365c3`） |

上記probeは文字・画像・gradient・clipを含まず、リポジトリにも入れていない。**G0の証拠には数えない。** 「同じLinux上で最小のSkia CPU経路がbuild・実行できた」という事実だけを示す。

### この環境で実施できないもの

macOS／Apple Silicon、WebView（WKWebView）、日本語IME、画面表示、App Sandbox、署名・notarization、offline release起動、プロセス単位の通信観測。これらは **ユーザーのMac実機** が必要であり、Step B として分離する。

## 3. 仕様との矛盾・リスク（実装前に把握すべき点）

1. **開発環境と対象OSの不一致。** 対象はmacOS／Apple Siliconだが、このコンテナはLinux x86_64。Step A（headlessのRust engine）はここで進められるが、Linuxでの出力hash一致はmacOSでの一致を意味しない（`02_ARCHITECTURE.md` 12節）。golden hashはOS／archごとに別記録する。
2. **rust-skiaのbinary取得はbuild時の通信。** 既定ではbuild scriptがGitHubからSkia binaryを取得する。製品runtimeの通信ではないが、再現性のため、取得したbinaryのURL・SHA-256・feature・Skia revisionを記録し、`SKIA_BINARIES_URL`（`file://`）等でオフライン再buildできる経路を確認する。未確認のまま「固定済み」と書かない。
3. **SkParagraphの既定fallback。** SkParagraphの `FontCollection` は既定でシステムフォントへfallbackしうる。TXT-004（無断置換禁止）を満たすため、`TypefaceFontProvider` に登録した hash 固定のfaceだけを使い、fallbackを無効化し、glyph coverageを事前検査する方針とする。実際に無効化できるかはStep Aで否定試験する。
4. **ICUデータ。** `textlayout` は改行等でICUを使う。ICUデータの同梱方式（埋め込みかファイルか）とUnicodeデータ版はplatformごとに確認が必要。未確認。
5. **日本語の禁則。** SkParagraphの改行はICU（UAX #14）準拠が基本で、JLREQの禁則を全て満たす保証はない。Phase 0では「どの文字列でどこが改行されたか」を記録する試験表を作り、不足分を明示する（追加実装はPhase 1）。
6. **schemaの範囲。** 現行の中核schemaにはclip専用group・放射グラデーション・pathノードがない。Phase 0の「clip」は `image.clipToBounds` で検証する。outlineロゴは `logo` ノード＋vector assetで扱い、安全なSVGサブセットから独自pathへ変換する。schema拡張はPhase 1で行う。
7. **sampleは描画不能が正しい。** sampleは素材・フォントがunresolved、variantがdraftのため、engineは `E_FONT_MISSING`／`E_ASSET_MISSING`／未承認variantで停止すべきだ。描画確認用には別のfixture文書（承認済みvariant、解決済みhash）を `fixtures/public-safe/` に作る。
8. **SVG parser。** `usvg` のような汎用SVG処理系は多くの機能を解決・変換するため、「未対応を黙って消さない」方針と相性を確認する必要がある。Phase 0は `roxmltree`（DTD既定拒否）で読み、要素・属性のallowlist外を全てerrorにする独自変換を提案する。

## 4. Phase 0 の縦切りスコープ

### Step A — headless engine（このコンテナで実施可能）

広いUIは作らない。CLIで1文書→1 PNGを通す。

```
Cargo.toml                      # workspace（依存versionをlock）
crates/design-model/            # schema対応のRust型、厳格なdeserialize、意味検証、issue型
crates/design-svg/              # 安全なSVGサブセット→独自path（allowlist外はE_SVG_UNSUPPORTED）
crates/design-text/             # hash固定font登録、fallback無効、coverage検査、fit候補走査、overflow判定
crates/design-render/           # Resolve→RenderPlan→Skia CPU raster→PNG、RenderProfile固定
crates/render-cli/              # siliq-render <doc.json> --assets <dir> --variant <id> --out <file.png>
fixtures/public-safe/           # 合成画像・手書きSVGロゴ・fixture文書（顧客素材なし）
tests/golden/                   # 承認済み基準画像とhash（OS/arch別）
tests/security/                 # 危険SVG・パス・壊れた画像の否定試験
tests/offline/                  # runtime依存にnetwork/AI系crateがないことの検査script
docs/adr/                       # RenderProfile、SVG方針、text stackのADR
reports/G0.md                   # 実測結果（実行したものだけ）
```

処理順は `02_ARCHITECTURE.md` 5節のとおり、構文・参照検証 → binding → variant解決 → geometry／crop → 文字形成・fit → preflight → RenderPlan → Skia描画 → encode。RenderPlan生成後は文字を再測定しない。

### Step B — macOS実機（ユーザーのMacが必要）

- Tauri 2の最小shell：1ウィンドウ、native bufferをraw byte IPCで表示、`documentRevision`／`requestId` で古いframeを破棄。
- 転送の実測：標準fixture寸法（1,080×1,350）でのnative描画→転送→表示の p50／p95。
- IME probe：日本語変換中／確定後の区別、二重挿入の有無（TXT-001の予備試験）。
- release build：App Sandbox、network entitlement不付与、offline初回起動、`lsof -i`／`nettop` 等によるapp＋workerの接続・待受観測。

Step BはStep AのG0部分報告の後に、別途承認を得て着手する。

## 5. fixture方針

| 種類 | 方針 |
|---|---|
| 画像 | スクリプトで合成したPNG（gradient＋格子＋透明縁）。生成scriptと出力hashを保存 |
| ロゴ | 手書きの小さなSVG（path／rect／circle／linearGradient）。危険版（script、外部href、foreignObject、DTD、深いuse）も否定試験用に用意 |
| フォント | **ユーザーの選択が必要。** 案1：OFLの日本語フォント（例：Noto Sans JP）をライセンス文と共にfixtureへ同梱。案2：bytesはcommitせず、ローカルパスとSHA-256だけを記録し、未配置時の試験は `E_FONT_MISSING` を確認 |
| 文書 | 承認済みvariantの描画用1件、overflow用、欠落font用、欠落asset用、draft variant用 |

## 6. 依存の提案（Step A）

runtime：`skia-safe`（`textlayout`。versionはStep A開始時に確認してlock）、`serde`、`serde_json`、`sha2`、`roxmltree`、`thiserror`。
dev／test：`jsonschema`（Rust）またはPython `jsonschema`によるschemaとRust型の照合、`proptest`。

導入しないもの：HTTP client（reqwest／hyper等）、AI／ML系crate、Node sidecar、Sharp／libvips、Tauriのhttp・websocket・updater・shell plugin。`tests/offline/` のscriptで `cargo tree -e normal` を検査し、該当crateがあれば失敗させる。

## 7. 検証コマンド（予定。まだ存在しない）

```
cargo fmt --check
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo run -p render-cli -- fixtures/public-safe/basic.json --variant square --out out/basic.png
scripts/repeat-render.sh 10     # 10回出力しhash一致を確認
tests/offline/check-deps.sh     # runtime依存にnetwork/AI系がないこと
```

実装時に実在するcommandだけをREADMEへ記録する。

## 8. Step A の合格基準

- 承認済みfixture文書から、合成画像（cover＋focal point＋clip）、日本語＋英語の見出し、outlineロゴ、linear gradientを含むPNGを出力できる。
- 同一build・同一環境で10回出力したPNGのSHA-256が一致する。
- 以下で**出力せずに停止**し、node／variant／code／remedyを返す：font欠落（`E_FONT_MISSING`）、glyph欠落（`E_GLYPH_MISSING`）、許可サイズ候補でも収まらない文字（`E_TEXT_OVERFLOW`）、asset欠落（`E_ASSET_MISSING`）、危険／未対応SVG（`E_SVG_UNSUPPORTED`／`E_EXTERNAL_REFERENCE`）、draft variant、許可ディレクトリー外のパス・`..`・symlink（`E_PATH_DENIED`）。
- system font fallbackが使われないことを、fixture fontに無い文字で確認する。
- 同梱sampleは上記の欠落errorで停止する。
- 出力画像は作者（ユーザー）が目視承認してから golden に登録する。

## 9. 承認が必要な事項

1. 本計画（Step A）への着手。
2. Rust workspace と上記依存の追加。
3. フォントfixtureの扱い（案1／案2）。
4. rust-skiaの配布binaryをbuild時に取得すること（開発時の通信。製品runtimeとは別境界）。
5. Step B のためのmacOS実機での作業（後日、別途）。

## 10. 撤回・再検討の条件

- SkParagraphでfallbackを無効化できない、またはglyph coverageを確実に検査できない場合：text stackのADRを作り、HarfBuzz直接利用等の代替案を比較する。機能追加へ進まない。
- 同一環境の反復出力でhashが一致しない場合：原因（encoder metadata、並列処理、未初期化領域等）を切り分けるまでgoldenを作らない。
- rust-skiaの配布binaryを固定・オフライン再buildできない場合：source buildの環境（Mac実機等）を用意する計画に切り替える。
- macOSでのbuffer転送がp95目標から大きく外れる場合：dirty tile・cache・解像度を検証し、結果をADRに残す。

## 11. 未実施

製品コード、Tauri shell、文字・画像・SVGを含む描画、性能測定、IME、sandbox、offline release、通信観測、PDF／SVG出力。いずれも結果は存在しない。
