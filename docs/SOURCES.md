# 技術根拠となる一次資料

参照確認日：2026-10-10。ここにある資料はライブラリーの能力・制限を確認するためのものだ。この設計の性能や完成を証明する資料ではない。versionを固定する段階で再確認する。

## S01 — Tauri：capabilitiesとCSP

`https://v2.tauri.app/security/capabilities/`

`https://v2.tauri.app/security/csp/`

capabilityはWebViewへ公開する権限を制約する。カスタムcommandの露出とRustの実装は別に検証する。CSPをnative側の通信遮断と混同しない。

## S02 — Skia：User Documentation

`https://skia.org/docs/user/`

2D図形・文字・画像の描画、rasterやPDF等のbackendを確認する。

## S03 — rust-skia：公式リポジトリー

`https://github.com/rust-skia/rust-skia`

Rust bindings、target、feature、native build、配布binaryとライセンス確認の入口。

## S04 — HarfBuzz：What HarfBuzz doesn't do

`https://harfbuzz.github.io/what-harfbuzz-doesnt-do.html`

文字形成と、段落の折り返し等の役割を分ける根拠。

## S05 — 日本語組版・Unicode

`https://www.w3.org/TR/jlreq/`

`https://www.unicode.org/reports/tr14/`

禁則、文字組み、line breakingの基礎資料。参照しただけで全要件を実装したことにはならない。

## S06 — Skia：PDF Backend

`https://skia.org/docs/user/sample/pdf/`

未対応、効果の省略・展開・raster化等の制限があるため、PDF出力の個別検証が必要。

## S07 — Skia Color Management

`https://skia.org/docs/user/color/`

color space、source／destination、表示と描画の色管理を確認する。

## S08 — Tauri core IPC

`https://v2.tauri.app/reference/javascript/api/namespacecore/`

Channelやraw byte／ArrayBuffer等の転送を確認。方式の存在と実測性能は別。

## S09 — Apple：Network client entitlement

`https://developer.apple.com/documentation/bundleresources/entitlements/com.apple.security.network.client`

macOS sandboxのnetwork entitlementの確認先。Host・worker・WebViewの実際の権限はrelease実機で確認する。CSPの代わりではない。

## S10 — Tauri native testing

`https://v2.tauri.app/develop/tests/webdriver/`

現行資料のWebdriverIO embedded方式と、tauri-driver直接利用のプラットフォーム差を確認する。test buildの機能は製品releaseへ入れない。

## S11 — Claude Code：project instructions

`https://code.claude.com/docs/en/memory`

CLAUDE.mdの役割と、強制的な設定・権限制約との違いを確認する。

## S12 — Tauri WebView

`https://v2.tauri.app/reference/webview-versions/`

OSごとのWebView依存を確認し、対象OSを未検証で拡大しない。
