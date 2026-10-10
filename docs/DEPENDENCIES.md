# 依存関係・配布物・再現条件

確認日：2026-10-10。Rust の直接依存は `=`、npm の直接依存は exact version で固定し、`Cargo.lock` と `package-lock.json` を保存する。第一者コードの `UNLICENSED` / `publish = false` は維持する。

## 共通エンジンと保持するホストの直接依存

| 依存 | 固定 version / feature | 用途 | upstream license |
|---|---|---|---|
| skia-safe / skia-bindings | 0.153.3、defaults 無効、binary-cache / embed-icudtl / pdf / jpeg / textlayout | CPU 正本描画、文字組み、PNG/JPEG/PDF | MIT、Skia 本体 BSD-3-Clause |
| serde / serde_derive | 1.0.229、std / derive | 厳格な JSON 型 | MIT OR Apache-2.0 |
| serde_json | 1.0.151、std | 文書・journal・manifest | MIT OR Apache-2.0 |
| sha2 | 0.11.0、defaults 無効 | 素材・出力・保存の整合性照合 | MIT OR Apache-2.0 |
| roxmltree | 0.21.1、std、positions 無効 | 読み取り専用 SVG parser | MIT OR Apache-2.0 |
| image-webp | 0.2.4、defaults 無効 | WebP 素材 decode / lossless derivative encode | MIT OR Apache-2.0 |
| zip | 8.6.0、defaults 無効 | Stored ZIP の `.siliqcg`、圧縮・暗号化なし | MIT |
| rustix | 1.1.5、defaults 無効、std / fs | FD に対する no-follow 読み書き・atomic publish・lock | Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT |
| tauri | 2.12.2、defaults 無効、wry / compression | 参照用WebViewホスト。製品C ABIからWebViewは初期化しない | Apache-2.0 OR MIT |
| tauri-plugin-dialog | 2.8.1、defaults 無効 | 参照用picker。製品画面のpickerはAppKit | Apache-2.0 OR MIT |
| base64 | 0.22.1 | 制限付きC ABI JSONへのcanonical PNG転送 | MIT OR Apache-2.0 |
| SwiftUI / AppKit | macOS 13以降のOS SDK | Mac製品画面・OS picker。外部Swift packageなし | OS framework（再配布しない） |
| libc | 0.2.190 | OS mount/grant 検査 | MIT OR Apache-2.0 |
| @tauri-apps/api | 2.12.2 | 制限付き native command 呼び出し | Apache-2.0 OR MIT |
| react / react-dom | 19.3.0 | ローカル UI | MIT |
| lucide-react | 1.55.0 | UI icons | ISC |

宣言の正本は各 `Cargo.toml` / `package.json`、解決済み推移的 version は lockfile。全推移的依存の宣言・本文位置・SHA-256 は `third_party/license-inventory.json` に収録する。proc-macro は normal edge 上に現れても実行時ライブラリではないため、SBOM で compile role を明示する。

CPU Skia の組は `jpegd-jpege-pdf-textlayout`。GPU / Skia SVG module / Skottie / Skia WebP は有効にしない。WebP は独立した固定 codec を使う。ICU data は `embed-icudtl` で実行ファイルに含める。システムフォントの列挙や暗黙 fallback を採用しない。SVG/PDF の derivative は native render から明示的な変換承認を受けて作る。

Mac製品画面は `apps/macos` のSwiftUI／AppKit、接続口は `siliq-native-bridge`。同じRustホストのplain helperを再利用し、WebView／server／pluginは初期化しない。C ABIは最大20 MiBのUTF-8 JSONと最大64 MiBの応答を扱い、型・command allowlist・重複キーを検査する。[ADR 0010](adr/0010-native-macos-interface.md)に移植理由を記録する。保持するホストのリンク指定にWebKit.frameworkが残る場合があるため、依存名の不在だけで通信禁止を証明しない。Swiftと製品バイナリーの直接WebView／network参照、実際のWebKit child起動の有無を別に検査する。

## 検証済み Skia 取得経路

`scripts/cargo-verified.sh` は `third_party/skia-binaries.lock` に従い archive の SHA-256 を検査し、ローカル `file://` URL を build script に渡す。素の Cargo では upstream build script の既定 download が動く可能性があるため、CI と desktop wrapper は検証済み経路を使う。ビルド時の依存取得と製品の実行時通信を区別する。

| target | rust-skia release / key | archive SHA-256 |
|---|---|---|
| x86_64-unknown-linux-gnu | 0.153.3 / b7f043e0b1e2a850e702-x86_64-unknown-linux-gnu-jpegd-jpege-pdf-textlayout | a23d6b205b630690e5f3c86ceaa5e3b6a1a3cb487bab18b8f27af0d2e3eef870 |
| aarch64-apple-darwin | 0.153.3 / b7f043e0b1e2a850e702-aarch64-apple-darwin-jpegd-jpege-pdf-textlayout | 75e65a6e374581d59b00519719db1ff4064c256aeac9c4df977366fbcf1ec047 |

取得元は [rust-skia 0.153.3 binary release](https://github.com/rust-skia/skia-binaries/releases/tag/0.153.3)。key 先頭の `b7f043e0b1e2a850e702` は rust-skia wrapper の source commit であり、Skia 本体の revision ではない。Skia は wrapper の `package.metadata.skia` が指定する `m153-0.101.2`、tag commit は `61e7ca4e99062cdd0ab69445d5963fb3365778f6`。正本は [rust-skia bindings manifest](https://github.com/rust-skia/rust-skia/blob/0.153.3/skia-bindings/Cargo.toml) と [Skia source tag](https://github.com/rust-skia/skia/tree/m153-0.101.2)。

archive に含まれる静的 library は Skia、SkParagraph、SkShaper、SkUnicode、bindings。Expat、HarfBuzz、ICU、libjpeg-turbo、libpng、Wuffs、zlib の本文は Skia tag の [DEPS](https://github.com/rust-skia/skia/blob/m153-0.101.2/DEPS) に指定された source revision から取得し、URL / revision / hash を `third_party/native-license-sources.json` に記録する。配布済み archive は全 C/C++ 入力の attestation を持たない。DEPS 記録は binary の全内容を証明するものではなく、native 成分の完全な provenance / CVE 検証は別途必要。

## ライセンスと SBOM

`third_party/NOTICE`、`third_party/licenses/`、`third_party/sources/`、各 inventory と target SBOM は macOS app の resources に含める。public-safe の Noto Sans JP には既存 OFL 本文を含め、利用者のフォントは許可情報に従う。`ReferenceOnly` フォントを共有用 project ZIP に埋め込まない。

通常の Cargo/npm runtime closure の全文を集める。公開 crate が本文を含めない場合は、元 commit の upstream 文書を保存する。objc2 系の完全本文補完は元 revision の宣言と区別し、補完元の immutable commit を記録する。MPL-2.0 の `selectors 0.38.0` については Cargo.lock の hash と一致する未改変 published source archive を `sources/` に同梱する。`deny.toml` の例外はこの version に限る。他のコードへの license の変更は行わない。

```sh
npm ci
npm run build
scripts/cargo-verified.sh check -p siliq-native-bridge --locked
python3 scripts/generate-sbom.py --package siliq-native-bridge --target aarch64-apple-darwin
```

生成器は開発時だけの Python 標準ライブラリ。自身では通信せず、準備済み Cargo/npm cache と検証済み Skia archive を読む。対象 package / target の normal graph に限定し、`cargo tree` の runtime feature context を使う。build/dev edge は除外し、proc-macro は明示する。CycloneDX 1.6 JSON に version、license 宣言、registry package checksum、workspace `Cargo.toml` + `src/*.rs` source fingerprint、dependency edges、lock hash を出す。SBOM の source hash と実行ファイル hash は用途が異なり、実行ファイル hash は bundle audit に記録する。OS framework、完全な native 内部 dependency graph、build toolchain はこの SBOM の対象外。

絶対パス、Cargo registry のローカル ID、認証付き source URL は出力しない。生成物は実行時の入力ではなく配布時の説明資料。同じ source/lock/target に対する再生成は同じ JSON になる。source 変更後は native build 前に再生成する。

## オフライン監査の範囲

`tests/offline/check-deps.sh` は core CLI の normal graph で network / async / TLS / AI / ML crate を禁止し、release CLI の直接 socket / DNS symbol と動的 library を検査する。Mac製品は `scripts/offline-desktop-audit.sh` で別に、native-bridgeのnormal feature graph、Rust facadeとSwiftのWebView／network／process API、command allowlist、実際の `siliq-studio` とdylibの署名・直接symbol・Sandbox entitlementを検査する。旧Tauriのlocal-only capabilityとCSPは参照用検査として区別する。

`scripts/test-macos-sandbox.sh` は追加依存なしのFoundation検証アプリを署名し、同梱のRust dylibと公開用fixtureを使って保存・再読込・5形式量産を実行する。製品と同じnetwork権限なしのentitlementsを実署名から検査し、専用app IDのコンテナ内でのみ作業する。静的グラフや通常のRustテストでは見つけられない、Sandbox内のファイルアクセス失敗を継続検証する。

Tauri は IPC / local CPU task のため Tokio 1.53.2 を要求する。現 target の normal tree は `bytes,default,fs,io-util,rt,rt-multi-thread,sync` で、`net` / `process` はない。Cargo metadata は build context の feature を統合するため、metadata だけで runtime network capability を判定しない。`deny.toml` の Tokio wrapper 例外は pinned Tauri の直接依存だけとし、runtime `net` / `process` 禁止は desktop audit で検査する。[cargo-deny wrapper の意味](https://embarkstudios.github.io/cargo-deny/checks/bans/cfg.html#wrappers)。HTTP / TLS / AI 関連の禁止を緩和しない。transitive `tauri-plugin-fs` は dialog の `FilePath` 型の依存であり、plugin を初期化せず frontend の fs capability を与えない。

Linux CLI の FreeType / Fontconfig と macOS の OS framework はシステム依存。旧 Linux fixture hash は当時の FreeType version 条件付きである。固定 Rust/Skia/lockfile は任意 OS/toolchain 間の byte-identical release を保証しない。native golden は target / backend / profile と併せて評価する。

Phase 0 の cargo-deny / RustSec の検査記録は `reports/G0-step-a.md` に残る。現 desktop 追加後の `cargo deny check` はこの端末で未実施であり、旧 pass を現 graph の検査結果として扱わない。RustSec のみでは native C/C++ とシステム library の CVE をカバーしない。静的 symbol / entitlement 検査は通信の実測ではなく、機内モードの初回起動・全操作試験と通信帰属の検査も別の受入条件となる。

## CI と開発環境

`.github/workflows/ci.yml` は UI / Linux core / macOS native に分け、Action commit SHA、Node 25.1.0、Rust 1.97.0、lockfile を固定する。UI は `npm ci`、契約再生成差分、型、テスト、build。Linux Ubuntu 24.04 x64はnative GTK/WebKitを宣言せず、siliq-desktopとsiliq-native-bridgeを除外してcoreを検査する。macOSは `macos-15` のARM64を実確認し、保持するReact資産をbuildしてからSwift型検査とworkspace全体のfmt／clippy／testを行う。SBOM、`npm run desktop:build` のSwiftUI release app、development用ad-hoc signatureと静的auditを検査する。

`macos-15` の ARM64 対応は [GitHub hosted runner 表](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) と [runner-images](https://github.com/actions/runner-images/blob/main/README.md) を 2026-10-10 に確認した。checksum-pinned Skia archive の対象と一致する。CI の ad-hoc signature は配布用 Developer ID / notarization の代わりにはならない。workflow 作成は GitHub hosted CI の実行成功を意味しない。

開発依存は製品 runtime と区別する。tauri-build 2.7.1 / Tauri CLI 2.12.1、TypeScript 7.0.2、Vite 8.3.4、Vitest 4.1.11、React plugin 6.1.2、testing-library 16.3.3、jsdom 27.4.0、json-schema-to-typescript 16.0.0、types React/DOM 19.3.0 は build/test のみ。Skia の bindgen/cc/clang/tar 等も build のみ。詳細 version の正本は lockfile。
