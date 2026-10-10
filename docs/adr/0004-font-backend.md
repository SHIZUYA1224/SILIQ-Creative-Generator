# ADR 0004 — フォントの読み込み経路（FreeType／プラットフォーム）

状態：Phase 0で採用、macOS実機で要検証／2026-10-10

## 背景

フォントは、作者が指定しhashで固定したファイルだけを使う。システムフォントの列挙やfallbackは一切使わない。そのためには、Skiaの「font manager」でbytesからtypefaceを作る必要がある。

調査で分かった事実：
- Linux版の公式Skia binaryはFreeTypeを使い、`SkFontMgr_New_Custom_Empty`（システムフォントを持たないFreeType manager）を利用できる。ただしFreeType自体は **システムの共有ライブラリー**（この環境では2.13.2）だ。
- **macOS版の公式Skia binaryはFreeTypeを含まない**（rust-skiaの `platform/macos.rs` で `uses_freetype = false`。archive内にFreeTypeシンボルが無いことも確認した）。macOSでは `FontMgr::custom_empty()` が `None` を返す。
- `embed-freetype` 付きの公式binary（key `ftembed`）は存在しなかった。

## 決定

`FontBackend` を2種類持ち、使った方をreportの `fontBackend` に必ず記録する。
1. `freetype-custom-empty`：使えるbuildでは常にこちら（Linux）。
2. `platform-from-data`：custom-emptyが無いbuild（macOS）。プラットフォームのmanager（macOSではCoreText）を **`new_from_data` だけ** に使う。`FontCollection` には登録しない。

どちらの経路でも次の3点は同じだ。
- `FontCollection` はdefault managerなし、fallback無効。
- 組版前に、全コードポイントのglyph有無を検査する（無ければ `E_GLYPH_MISSING`）。
- 組版後に、全runが指定typefaceであること、unresolved glyphが0であることを検査する。

## 検証状況

- Linuxでは両経路をテストした（`platform_font_backend_keeps_guards_and_determinism`、`post_layout_guard_detects_unresolved_glyphs`）。Linuxのplatform経路はfontconfig＋FreeTypeであり、CoreTextではない。
- **macOS（CoreText経路）は未検証。** CoreTextでラスタライズしたglyphはLinuxのFreeTypeと画素が一致しない見込みなので、golden hashはOS／arch別に持つ。
- 撤回条件：macOSでCoreText経路がfallbackを起こす、glyph検査をすり抜ける、または同一環境の反復で一致しない場合。そのときはSkiaをFreeType込みでsource buildする計画（Mac実機が必要）へ切り替える。
