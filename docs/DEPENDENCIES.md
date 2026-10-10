# 依存関係と採用理由（Phase 0 Step A）

確認日：2026-10-10。versionは `Cargo.toml` で `=` 固定し、`Cargo.lock` をcommitしている。

## 1. Rust runtime依存（製品バイナリーに入るもの）

| crate | version | 有効feature | 採用理由 | license |
|---|---|---|---|---|
| skia-safe / skia-bindings | 0.153.3 | `binary-cache`, `embed-icudtl`, `pdf`, `jpeg`, `textlayout`（default-features無効） | 共通2D描画、SkParagraph（文字組み）、PNG/JPEG codec | MIT（bindings）／Skia本体はBSD-3-Clause |
| serde / serde_derive | 1.0.229 | `std`, `derive` | 文書JSONの厳格な読み込み（`deny_unknown_fields`） | MIT OR Apache-2.0 |
| serde_json | 1.0.151 | `std` | JSON parser | MIT OR Apache-2.0 |
| sha2 | 0.11.0 | なし（default無効） | 素材・フォントのSHA-256照合、出力hash | MIT OR Apache-2.0 |
| roxmltree | 0.21.1 | `std`（`positions`無効） | SVGの読み取り専用XML parser。DTDは既定で拒否 | MIT OR Apache-2.0 |

推移的runtime依存（`tests/offline/check-deps.sh` の出力）：bitflags, block-buffer, cfg-if, cpufeatures, crypto-common, digest, hybrid-array, itoa, memchr, serde_core, typenum, zmij と、proc-macroのproc-macro2 / quote / syn / unicode-ident。

### skia-safeのfeatureについて

公式の配布binaryは決まったfeatureの組だけが公開されている。2026-10-10時点で `textlayout` を含み、GPUを含まない組として存在を確認できたのは `jpegd-jpege-pdf-textlayout` だけだった（`textlayout` 単独、`jpegd-textlayout`、webp入りの組は404）。そのため `pdf` と `jpeg-encode` もSkia binaryに含まれる。Phase 0のコードはPDF出力もJPEG出力も呼ばない。WebP decoderはこのbinaryに含まれないため、WebP素材は `E_ASSET_TYPE` で拒否する。

`embed-icudtl` はICUデータを実行ファイルへ埋め込み、実行時にデータファイルや取得を必要としないために有効化している。

GPU（`gl`／`metal`／`vulkan`）、`svg`（SkiaのSVGモジュール）、`skottie`、`webp` は有効化していない。

## 2. Skia native binary

rust-skiaのbuild scriptは配布binaryを **整合性検査なしで** 取得する（`download.rs` に `TODO: verify key`）。このため `scripts/cargo-verified.sh` が、公式URLから取得したarchiveのSHA-256を `third_party/skia-binaries.lock` と照合する。その後、`SKIA_BINARIES_URL=file://…` でbuildへ渡す。

| target | 取得元 | SHA-256 |
|---|---|---|
| x86_64-unknown-linux-gnu | `https://github.com/rust-skia/skia-binaries/releases/download/0.153.3/skia-binaries-b7f043e0b1e2a850e702-x86_64-unknown-linux-gnu-jpegd-jpege-pdf-textlayout.tar.gz` | `a23d6b205b630690e5f3c86ceaa5e3b6a1a3cb487bab18b8f27af0d2e3eef870` |
| aarch64-apple-darwin | `…/skia-binaries-b7f043e0b1e2a850e702-aarch64-apple-darwin-jpegd-jpege-pdf-textlayout.tar.gz` | `75e65a6e374581d59b00519719db1ff4064c256aeac9c4df977366fbcf1ec047` |

- Linux版は2回独立に取得し、同じhashになった。macOS版はhashを記録しただけで、build・実行は未実施。
- rust-skiaのSkia fork revisionはkey先頭の `b7f043e0b1e2a850e702`。
- 素の `cargo build` は rust-skia の既定動作でGitHubから **未検証のまま** 取得する。検証済み経路は `scripts/cargo-verified.sh` だけだ。
- archiveの中身：`libskia.a`、`libskparagraph.a`、`libskshaper.a`、`libskunicode_core.a`、`libskunicode_icu.a`、`libskia-bindings.a`、`bindings.rs`、`LICENSE_SKIA`。
- `libskia.a` 等には第三者コードが静的に含まれる。シンボルで存在を確認したもの：HarfBuzz、ICU、libpng、libjpeg-turbo、wuffs、expat。配布時にはこれらのlicense表示が必要になる（G4で整備。Phase 0では未作成）。

## 3. 実行ファイルの外部依存（実測）

`target/release/siliq-render`（Linux x86_64）が直接要求する共有ライブラリー（`readelf -d` のNEEDED）：

```
libstdc++.so.6, libfontconfig.so.1, libfreetype.so.6, libgcc_s.so.1, libm.so.6, libc.so.6, ld-linux-x86-64.so.2
```

`ldd` では、さらにlibz、libbz2、libpng16、libbrotlidec、libbrotlicommon、libexpatが間接的に読み込まれる。

- `libfontconfig` は、macOS向けに用意したplatform font経路（`FontMgr::new()`、ADR 0004）を参照するためにリンクされる。既定のFreeType custom-empty経路では、fontconfigの設定ファイルやフォントディレクトリーを一切開かない（strace実測。`reports/G0-step-a.md` §6）。
- Linux版のSkiaは **システムのFreeType** を使う。この環境では `libfreetype6 2.13.2+dfsg-1ubuntu0.1`。`nm -D` の未定義シンボルのうち直接参照しているのは `FT_*` だけだ。libpng／zlib／bz2／brotliはlibfreetypeの依存として読み込まれる。SkiaのPNG codecは静的に内蔵されている。
- **影響：glyphのラスタライズ結果はシステムのFreeTypeに依存する。** golden hashは「このFreeType版」の条件付きで有効。公式binaryに `embed-freetype` 版（key `ftembed`）は存在しなかった（404）。
- socket／DNS系シンボル（`socket`、`connect`、`getaddrinfo` 等）はimportしていない（`tests/offline/check-deps.sh`）。

## 4. 開発時だけの依存

| 用途 | 内容 |
|---|---|
| build | bindgen、cc、clang-sys、flate2、tar など（skia-bindingsのbuild依存。製品バイナリーには入らない） |
| 監査 | cargo-deny 0.20.2（`deny.toml`）。RustSec advisory-db `7eebec69c352c7191b1f13eb95dd510eeca5d1de`（2026-10-09） |
| fixture生成 | Python 3標準ライブラリーのみ（`scripts/gen_fixture_photo.py`） |
| toolchain | Rust 1.97.0（`rust-toolchain.toml`） |

## 5. 監査結果（2026-10-10、Linux x86_64）

`cargo deny check` の結果は `advisories ok, bans ok, licenses ok, sources ok`。

- advisories：RustSecに該当するadvisoryなし（yankedも含む）。
- licenses：許可リストは MIT、Apache-2.0、Apache-2.0 WITH LLVM-exception、BSD-3-Clause、ISC、Unicode-3.0、Zlib、Unlicense。
- bans：network client、async runtime、TLS、AI／ML runtime系crateの禁止リストに該当なし。重複versionの警告が2件（`shlex` 1.3/2.0、`syn` 2/3。いずれもbuild／proc-macro側）。
- sources：crates.ioのみ。

**未実施：** Skia binaryに静的に含まれるC/C++ライブラリー（HarfBuzz、ICU、libpng、libjpeg-turbo、expat、wuffs）とシステムFreeTypeについて、CVEとの照合はしていない。RustSecはRust crateだけを対象とするため、上の結果はこれらをカバーしない。
