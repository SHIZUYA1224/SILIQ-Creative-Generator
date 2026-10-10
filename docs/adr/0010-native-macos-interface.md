# 0010 — macOS のネットワーク権限を持たないネイティブ画面

Date: 2026-10-10. Status: accepted for the Mac implementation.

## 実機で分かった制約

Tauri の画面を同梱した release `.app` に App Sandbox と user-selected read-write だけを署名すると、ローカル作業領域の準備後に WKWebView が白画面になった。WebContent / Networking service が繰り返し終了し、OS ログに `Application does not have permission to communicate with network resources` が記録された。開発サーバーは起動していない。

[WebKit の公式実装](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/Shared/EntryPointUtilities/Cocoa/XPCService/XPCServiceEntryPoint.mm) は、sandboxed client が network.client entitlement も nsurlsessiond の mach lookup 許可も持たない場合に起動を拒否する。file URL に対しても過剰な要件であることを同実装が注記している。[Tauri の追跡 issue](https://github.com/tauri-apps/tauri-docs/issues/3171) にも同じ挙動が報告されている。

network.client、広い mach exception、Sandbox の解除は製品条件を満たさないため採用しない。静的署名検査だけでは実機の起動成功を証明できない。

## 採用する構成

Mac の製品画面を SwiftUI / AppKit に移し、WKWebView を初期化しない。共通の Rust 文書、typed command、revision / transaction、Skia CPU、保存、復旧、grant、量産 worker は再利用する。`siliq-native-bridge` は bounded UTF-8 JSON の C ABI を提供し、ネットワークや localhost server を開始しない。画面は Swift actor を通して C ABI を順次呼ぶ。

OS picker と security-scoped URL の寿命は Swift が保持する。Rust は選択された通常ファイルを、Darwin の metadata-only 検査と `O_NOFOLLOW_ANY`、held directory descriptor により検査・読み書きする。量産では出力 URL の scope を worker / 中断ジョブの参照中も保持し、再起動後は再選択する。

正確プレビューは共通 renderer の canonical PNG で、AppKit は画像として表示する。選択、ガイド、ハンドルは操作表示であり、文字組みや作品の描画を再実装しない。

React / Tauri の画面とテストは既存の編集契約の実装・参照として保持する。Mac の製品起動と配布検証の対象は SwiftUI の `.app` である。Rust facade は同じ host の plain helper を使用するため、依存 inventory に Tauri の crate が含まれても WebView を起動することを意味しない。

## 検証条件

C ABI の unit / integration、Swift の型検査、生成した bundle の署名・entitlement・依存検査を行う。さらに実際の Sandbox bundle で起動、編集、Undo、保存・再読込、画像 / PDF、量産と再開を検査する。通常の CLI 性能値を UI 転送や native responder の性能に読み替えない。公証と一般公開署名は別の配布条件である。
