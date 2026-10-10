# 06 — 決定事項・リスク・変更管理

## 1. 設計決定

| ADR | 決定 | 理由／代替案 |
|---|---|---|
| ADR-001 | 完全オフライン、実行時AI禁止 | 製品の絶対条件。オンライン／ローカルLLMは代替案にしない |
| ADR-002 | 独自エディター＋量産エンジン | ユーザーがBを選択。単純な差替ツールやIllustrator依存へすり替えない |
| ADR-003 | 独自文書を正本とする | SVGやCanvas libraryの内部JSONだけでは製品固有の制約・job等を扱えない |
| ADR-004 | Tauri＋Rust＋Skiaを本線 | Native共通描画とReact UIを分離。Phase 0で成立性を測る |
| ADR-005 | Native CPU経路を画質基準 | GPU差分を最初から再現性保証の前提にしない |
| ADR-006 | 文字形成と段落・fitの責務を分離 | HarfBuzz採用だけで日本語組版完成としない |
| ADR-007 | SVG／PDFは交換・派生出力 | 保存の完全性と外部形式の再現性を分離 |
| ADR-008 | 作者承認済みvariantだけ生成 | サイズ違いで無断デザイン変更をしない |
| ADR-009 | 入力不足・未対応はerrorを基本 | 黙った置換・省略・低画質化による量産事故を防ぐ |
| ADR-010 | Windows・商業印刷は追加ゲート | 初期検証はMac・sRGB。無検証の万能性を約束しない |
| ADR-011 | フォントbytesを無断で配布しない | 見た目と同梱権利を別に管理 |
| ADR-012 | releaseの実機試験を必須にする | browser mockやdev buildだけでは権限・IME・通信を証明できない |

## 2. 旧案からの変更点

元スターターの再現性、schema独立、画像・SVG検証、フォント欠落検出、未実装ボタン禁止という方向は維持する。

一方、local-firstはoffline-onlyへ強化する。最終SVG中心の経路は独自文書→共通native render経路へ置き換える。Sharp／libvipsを最初から入れず、同じ描画に複数のnative／Node基盤を持つ構成を避ける。AI禁止は外部APIだけでなくローカル推論まで拡張する。

ブラウザーのPlaywright画像だけでnative完成判定を行わない。SVGを「全て編集可能な完全保存形式」と呼ばない。PDFを「出力できるから商業入稿対応」と扱わない。

## 3. 最大リスク

| リスク | 影響 | 先行検証／軽減 |
|---|---|---|
| native buffer転送・表示が遅い | 編集体験が悪い | Phase 0で実測、dirty tile、cache、revision取消 |
| 日本語IMEとnative glyph情報の整合 | 文字編集不能・データ破損 | UTF-16/UTF-8/grapheme変換、composition試験 |
| Skia／文字stackのnative build | 特定実機で起動不能 | target固定、feature監査、clean build、依存pin |
| SVG／PDFの機能差 | 納品物の欠落 | 対応matrix、preflight、承認済みraster化、再描画比較 |
| フォントの版違い・不足 | 字幅・改行・字形が変わる | hash／face固定、fallback禁止、明確な停止 |
| unsafeなnative parser入力 | crash・侵害 | 上限、更新、worker隔離、sandbox、fuzz／否定試験 |
| OS sandboxと選択ファイル権限の不整合 | 保存失敗か過剰権限 | grant broker、実機の拒否試験、許可取消 |
| ZIP保存・大量出力中の停止 | 原本・成果物の破損 | 原子的保存、journal、snapshot、checksum付きresume |
| GPU・色変換・codec版の違い | 出力のばらつき | CPU基準、RenderProfile、版固定、環境別比較 |
| 機能を広げすぎる | 核心が未完成になる | G0→G4、未実装UI禁止、非目標の維持 |

## 4. 未確定事項

ユーザー指定として未確定なのは、対象Macの機種／OS、実際の代表デザインと素材、フォントの種類と持出条件、最大生成件数、必須の広告・印刷寸法、商業印刷対応の必要時期だ。

現在は、Mac／Apple Silicon、16GB、デジタル静止画sRGB、日本語・英語横組みを検証用の初期範囲として提案している。未確定だから設計を止めるのではなく、差し替え可能な設定とリスクとして管理する。

技術的に未確定なのは、互換version、Skia feature flags、内部surface精度、native buffer経路の実測、native E2E harness、release sandboxの具体entitlement、font同梱方式、export対応matrixの詳細だ。これらはPhase 0の成果物で確定する。

## 5. 変更してよいもの／いけないもの

ユーザーの承認なしに変更してはいけないもの：実行時AI禁止、外部通信禁止、独自エディター方式、指定外デザイン変更禁止、機密素材の外部送信禁止。

実測に基づいてADRで変更できるもの：native buffer transport、cache構造、crate分割、codec実装、内部precision、UI libraryの補助部品、試験harness。

v1の対象機能を減らす場合、スコープを変更したpreview版として明示する。未達を隠して完成扱いにしない。性能目標を緩和する場合は、実測結果と理由を記録して合意を取る。
