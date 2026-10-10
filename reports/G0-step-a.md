# G0 部分報告 — Phase 0 Step A（headless CPU描画エンジン）

報告日：2026-10-10。対象：`docs/PHASE0_PLAN.md` の Step A。**Step B（macOS実機、Tauri、IME、sandbox、offline release）は未着手。G0の合格判定ではない。**

## 0. 要約

- 1つのJSON文書から、PNG写真（cover＋焦点＋clip＋透明な縁）、日本語＋英語の文字（Noto Sans JP、承認済みサイズ内のfit）、安全サブセットのSVGロゴ（グラデーション、even-odd、変形）、線形グラデーション、回転・半透明groupを描いてPNG出力した。
- 自動テスト98件がすべて成功（失敗0、ignore 0）。
- 同一build・同一環境（Linux x86_64）で、正方形・縦長それぞれ10回連続生成したPNGのSHA-256が10/10一致した。
- フォント欠落、glyph欠落、文字あふれ、危険SVG、許可外パス、上書き、未承認variantは、出力せずに停止することをテストで確認した。
- **macOSの公式Skia binaryにはFreeTypeが無い** ことが判明した。そのためmacOSでは別のフォント読み込み経路（CoreText）になり、その経路は未検証（ADR 0004）。
- Linux版の文字のラスタライズは **システムのFreeType 2.13.2** に依存する（ADR 0004、docs/DEPENDENCIES.md §3）。

## 1. 実行環境

| 項目 | 値 |
|---|---|
| 実機 | Claude Codeのクラウドコンテナ。Linux 6.18、x86_64、4コア、約15GiB。macOSではない |
| toolchain | Rust 1.97.0（`rust-toolchain.toml`） |
| Skia | skia-safe／skia-bindings 0.153.3。公式binary `…x86_64-unknown-linux-gnu-jpegd-jpege-pdf-textlayout`（SHA-256照合済み） |
| FreeType | system `libfreetype6 2.13.2+dfsg-1ubuntu0.1` |
| build | debug（テスト）と release（CLI）の両方 |

## 2. 実装したファイルと役割

| パス | 役割 |
|---|---|
| `Cargo.toml`、`Cargo.lock`、`rust-toolchain.toml`、`rustfmt.toml` | workspace定義、依存の `=` 固定、lockfile、toolchain固定 |
| `crates/design-model/src/types.rs` | schema 0.2.0-core に対応するRust型。全structで未知フィールドを拒否。nullableなhashキーも必須 |
| `crates/design-model/src/validate.rs` | 範囲・形式検査と意味検証（ID重複、参照切れ、循環、孤立ノード、slot型、fitサイズ規則、hashと状態の整合）、文書サイズ上限 |
| `crates/design-model/src/variant.rs` | variantのpatch適用（bounds／fontSizeだけ）、未定義＝`E_LAYOUT_UNDEFINED`、未承認＝`E_VARIANT_NOT_APPROVED` |
| `crates/design-model/src/issue.rs` | 統一issue形式（severity、code、message、location、remedy） |
| `crates/design-svg/src/{parse,path,transform}.rs` | SVG安全サブセット→独自path。allowlist外は全てerror（ADR 0002） |
| `crates/design-render/src/resources.rs` | 内容アドレス型の資源読み込み（hash名のみ、symlink拒否、サイズ上限、SHA-256再照合） |
| `crates/design-render/src/raster.rs` | PNG／JPEGの内容識別、宣言型との照合、寸法の事前検査、sRGBへのdecode |
| `crates/design-render/src/text.rs` | フォント読み込み（hash固定、fallback無効）、glyph網羅検査、承認済みサイズでのfit、組版後の第二検査、改行範囲の記録 |
| `crates/design-render/src/lib.rs` | 描画パイプライン（検証→variant→資源→RenderPlan→Skia CPU→PNG）、RenderProfile、出力report |
| `crates/render-cli/src/main.rs` | `siliq-render check／render`。出力ディレクトリーgrant、ファイル名検査、上書き禁止、一時ファイル→hard linkでの公開 |
| `crates/*/tests/*.rs` | テスト（§4） |
| `fixtures/public-safe/` | 合成写真PNG、手書きSVGロゴ、Noto Sans JP＋OFL、fixture文書（README記載のhash付き） |
| `scripts/cargo-verified.sh`、`third_party/skia-binaries.lock` | Skia binaryのSHA-256照合付きbuild |
| `scripts/repeat-render.sh` | N回連続生成とhash比較 |
| `scripts/gen_fixture_photo.py` | 合成写真の生成（標準ライブラリーのみ） |
| `tests/offline/check-deps.sh` | runtime依存とバイナリーのimportに通信・AI系がないことの検査 |
| `deny.toml` | cargo-denyの方針（license、advisory、禁止crate、取得元） |
| `docs/DEPENDENCIES.md`、`docs/adr/0001〜0004` | 依存と採用理由、描画規約、SVGサブセット、issue code、フォント経路 |
| `reports/g0-step-a/` | 実際の出力PNGとreport JSON |

## 3. 実際のPNG出力

| ファイル | 寸法 | SHA-256 |
|---|---|---|
| `reports/g0-step-a/basic-square.png` | 1080×1080 | `01aadcd32973630e0b6a997668923a6b7ef4c0b8902113c28a4904cfa59348c7` |
| `reports/g0-step-a/basic-portrait.png` | 1080×1350 | `3104f902fe6755d9ef1996368efec528cbc82fb016b8243d041d7ccf94e41a9d` |

各PNGに対応する `*.report.json` には、使用したフォント／素材のhash、RenderProfile、font backend、文字ごとの試行サイズ・採用サイズ・改行範囲、出力hashが入っている。

目視で確認した点：写真のcoverとclip（枠外は描かれない）、透明な縁、焦点（円が枠内の65%／40%付近）、回転＋85%不透明のgroup、日英混在の見出し、右揃えのキャプション、ロゴの中抜き（even-odd）とグラデーション。縦長版は60ptで組まれ、ロゴとキャプションの位置もpatchどおりだった。

これらは **作者による目視承認を受けていない**。そのため `tests/golden/` には登録していない。

停止例（同じCLIで実行し、PNGは作られない）：
- 製品sample → `E_VARIANT_NOT_APPROVED`（variantを承認済みにしても `E_FONT_MISSING` と `E_ASSET_MISSING`×2）
- fixture文書の `wide-draft` → `E_VARIANT_NOT_APPROVED`

## 4. テスト結果

`scripts/cargo-verified.sh test --workspace`（2026-10-10、Linux x86_64、debug build）：

| テスト | 件数 | 結果 |
|---|---|---|
| `design-model` tests/model.rs | 32 | 32成功 |
| `design-svg` tests/svg.rs | 25 | 25成功 |
| `design-render` tests/render.rs | 33 | 33成功 |
| `design-render` unit（組版後の検査） | 1 | 1成功 |
| `render-cli` tests/cli.rs | 7 | 7成功 |
| **合計** | **98** | **98成功、失敗0、ignore 0** |

あわせて、`cargo fmt --check` 合格、`cargo clippy --workspace --all-targets -D warnings` 合格。`cargo deny check` は `advisories ok, bans ok, licenses ok, sources ok`（重複versionの警告2件）。`tests/offline/check-deps.sh` も合格。

主な否定テスト：未知プロパティ、未対応schemaVersion、範囲外の値、フォントのURLプロパティ、hashの欠落・大文字、ID重複、循環、孤立、slot型不一致、fitサイズの順序・範囲外、未承認・未定義variant、端数のある出力寸法、未解決／不在のフォント、フォントbytesの改ざん、存在しないface、glyph欠落（タイ文字、ハングル、絵文字）、制御文字、文字あふれ（error方針、全サイズ試行後、手動改行による行数超過）、素材の型偽装、壊れたPNG、WebP、寸法超過、危険SVG（script、外部参照、text、stroke）、ロゴ最小幅、symlink（ファイル、ディレクトリー、root）、hash名以外の読み込み、出力名のトラバーサル・絶対パス・予約名、既存ファイルの上書き、symlinkの出力先・文書。

## 5. 日本語描画・文字欠落の検証

**日本語描画**：Noto Sans JPで、和文・欧文・約物の混在、全角句読点、`𠮷`（サロゲートペア）の描画を確認した。改行範囲はSkParagraphがUTF-16単位で返すことをテストで確かめ、UTF-8のbyte位置へ変換して記録している。

**fit**：32文字の仮名を64→63→62→61で試して収まらず、60で2行に収まった。試行列 `[64, 63, 62, 61, 60]`、採用値60がreportに残る。40文字では56まで全サイズを試したうえで `E_TEXT_OVERFLOW` で停止した。

**文字欠落**（すべて出力なしで停止し、該当node／font／コードポイントを報告する）：

| 入力 | 結果 |
|---|---|
| `タイ語ภาษาไทย` | `E_GLYPH_MISSING`：U+0E17, U+0E20, U+0E22, U+0E29, U+0E32, U+0E44 |
| `Hangul 한국어` | `E_GLYPH_MISSING`：U+AD6D, U+C5B4, U+D55C |
| `絵文字😀` | `E_GLYPH_MISSING`：U+1F600 |
| `tab\there` | `E_GLYPH_MISSING`（制御文字） |
| フォントファイルなし | `E_FONT_MISSING`（代替フォントは使わない） |

fallback防止は二重にしている。①組版前にtypefaceのcmapで全コードポイントを検査する。②組版後に、全runが指定typefaceであり、unresolved glyphが0であることを検査する。②は①を通さない状態でも単独で働くことを、unit testで両方のfont backendについて確認した。

**禁則（観察結果、回帰テスト化済み）**：200du幅・40ptで1行5字の条件。

| 入力 | 改行結果 | JLREQとの関係 |
|---|---|---|
| `…お。か…` 系（。、」）！？・…々） | `あいうえ／お。かきく／けこ`（追い出し） | 行頭禁則を満たす |
| `…え「お…`、`…え（お…` | `あいうえ／「おかきく／けこ` | 行末禁則を満たす |
| `…おー…`、`…おゃ…`、`…おっ…`、`…おァ…` | `あいうえお／ーかきくけ／こ` | **差あり**：厳格な禁則では行頭不可（ICUの標準規則の挙動） |
| `価格は1,980円です。` | `価格は／1,980円で／す。` | 数字の分割なし |

小書き仮名と長音の行頭禁則は、Phase 1で製品側の規則として追加する必要がある（TXT-003）。

## 6. 10回連続生成のハッシュ比較

`scripts/repeat-render.sh 10 <variant>`（release CLI。毎回新しい出力ディレクトリー、別プロセス）：

| variant | 結果 | SHA-256 |
|---|---|---|
| square | 10/10一致 | `01aadcd32973630e0b6a997668923a6b7ef4c0b8902113c28a4904cfa59348c7` |
| portrait | 10/10一致 | `3104f902fe6755d9ef1996368efec528cbc82fb016b8243d041d7ccf94e41a9d` |

補足：
- debug buildのテスト内で10回レンダリングした結果も一致した（`repeated_renders_are_byte_identical`）。
- debug buildとrelease buildの出力hashも同一だった。
- toolchainを入れ直して再buildした後も同じhashだった。
- PNGにはtext／time／EXIFチャンクが無く、sRGBまたはICCの色情報はある（テストで確認）。
- **この一致は「同じLinux x86_64、同じSkia binary、同じシステムFreeType 2.13.2」の条件付き。** macOSや別のFreeType版で同じhashになるとは主張しない。

**通信とファイルアクセスの観測（Linux、参考値）**：
- `unshare -rn` でネットワークの無い名前空間に入れて実行すると、正常に描画でき、hashも同一だった。
- `strace -e trace=network` で観測したネットワーク系システムコールは0件。
- `strace` で観測したファイルアクセスは、文書、資源ディレクトリー内のhash名ファイル3つ、出力ディレクトリー（一時ファイルと最終ファイル）、パス正規化のための親ディレクトリーのstat、共有ライブラリー、`/proc/self/maps` だけだった。`/etc/fonts` やシステムフォントのディレクトリーは開いていない。
- これはmacOSのrelease検証（T-OFF-01/02）の代わりにはならない。

## 7. セキュリティ上の制限（Step A時点）

- **資源アクセス**：hash名（小文字64桁）＋固定拡張子だけを読む。symlink（ファイル、`assets/`・`fonts/`、root）は拒否する。正規化後のパスがroot配下にあることを再確認し、サイズ上限（画像100MB、フォント64MB、SVG 10MB、文書20MB）を読み込み前に検査する。
- **出力**：許可したディレクトリー直下にだけ書く。ファイル名は `[A-Za-z0-9._-]`、先頭は英数字、`..` 不可、拡張子は固定、Windows予約名は不可。一時ファイルを `create_new` で作り、fsyncしてから `hard_link` で公開するので、既存ファイルは上書きしない（競合時は `E_OUTPUT_COLLISION`）。
- **画像**：ヘッダーで型を識別し、宣言型と一致する1種類のdecoderだけを使う。寸法（各辺16,384px、64MP）はdecode前に検査する。アニメーションとEXIF回転は拒否する。
- **SVG**：DTD禁止、node数・深さ・数値・segment数の上限、外部参照の優先検出（ADR 0002）。
- **残るリスク**：
  - Skia内のC/C++ parser（libpng、libjpeg-turbo、FreeType、HarfBuzz、ICU）は同一プロセスで動く。worker分離・sandbox・fuzzはまだ無い。
  - 資源のhash照合はファイルを読み終えた後で行う。読み込み中に差し替える攻撃（TOCTOU）への対策は「symlink拒否＋正規化の確認」までで、OSのhandleを用いた相対操作ではない。
  - checksumは改ざん検出であり、出所の証明ではない。
  - ネイティブライブラリーのCVE照合は未実施。

## 8. 未対応機能（Step A時点で、明示的なerrorか機能なし）

- 入力：WebP（このSkia binaryにdecoderが無い）、EXIF回転、アニメーション画像、HEIC等。
- SVG：stroke、arc、clipPath、mask、filter、`use`、`text`、radialGradient、CSS、group opacity（ADR 0002）。
- 文字：小書き仮名と長音の厳格な行頭禁則、縦組み、ルビ、OpenType featureの指定、文字単位のスタイル、縦方向の配置指定、擬似bold／italic（全て未実装）。
- 配置：ロゴの `clearSpace` 検査（未実装）、ロゴ・文字の配置基準の選択（ADR 0001で固定）。
- データ：slotへのdataset binding、バッチ、manifest、resume（Phase 3）。
- 出力：JPEG／WebP／SVG／PDF出力、scale≠1の出力。
- 保存形式（`.siliqcg`）、編集、Undo、UI（GUIはスコープ外）。

## 9. Mac実機で検証すべき項目

1. `scripts/cargo-verified.sh build --release -p render-cli` がaarch64-apple-darwinでbuildでき、Skia binaryのhashが `75e65a6e…c047` で一致すること。
2. フォント経路が `platform-from-data`（CoreText）になること（reportの `fontBackend`）。そのうえで、glyph欠落・フォント欠落のテストがmacOSでも全て通り、fallbackが起きないこと（ADR 0004）。
3. `cargo test --workspace` の全件。ただし画素値を見るテストは許容差内か確認する。
4. `scripts/repeat-render.sh 10 square／portrait` が同一Mac上で10/10一致すること。hashはLinuxと異なる見込みなので、Mac用として別に記録する。
5. 改行表（§5）がCoreText経路でも同じになるか。異なる場合は記録してADRを更新する。
6. `otool -L` で外部依存を確認し、システムのフォントや通信に関わるframeworkがあれば列挙する。
7. ネットワークを遮断した状態での実行と、プロセス単位の通信観測（`nettop`／`lsof -i`、Little Snitch等）。
8. Step B：Tauriでnative bufferを転送・表示し、その速度を測る。日本語IMEのcomposition、App Sandbox（network entitlementなし）、署名、offline release初回起動。

## 10. 未実施

macOSでのbuild・実行、Tauri shell、IME、sandbox、署名・notarization、offline releaseの初回起動、性能測定（標準fixture、p50／p95、メモリー）、作者によるgolden画像の承認、ネイティブライブラリーのCVE照合、fuzzing、Windows。

## 11. 再現コマンド

```sh
scripts/cargo-verified.sh test --workspace
scripts/cargo-verified.sh clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
scripts/cargo-verified.sh build --release -p render-cli
scripts/repeat-render.sh 10 square
scripts/repeat-render.sh 10 portrait
tests/offline/check-deps.sh
cargo deny check            # cargo-deny 0.20.2
./target/release/siliq-render render --doc fixtures/public-safe/documents/basic.json \
  --variant square --resources fixtures/public-safe/resources \
  --out-dir out --name basic-square.png --report basic-square.report.json
```
