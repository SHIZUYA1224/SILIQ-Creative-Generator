# 02 — アーキテクチャ

## 1. 結論

**Tauri 2＋React／TypeScriptの操作UIと、Rust＋Skiaの共通制作エンジンを分ける。**

SVGやブラウザーDOMを作品の正本にしない。独自のバージョン付き文書を正本とし、文字組み・レイアウト・描画をRust側へ集約する。CanvasやDOMは画面の表示・入力・選択枠に使用しても、作品の最終位置や字形を決める権威にはしない。

TauriにはWebView側のAPI露出を制限するcapabilityがある。Skiaは文字・図形・画像を扱う2D描画基盤であり、Rust bindingsが提供されている。ただし、これらの存在がこの製品の性能・品質を証明するわけではない。採用条件はPhase 0の実機検証とする。[S01][S02][S03]

## 2. 構成

```text
┌──────────────────────────────────────────────┐
│ React / TypeScript UI (Tauri WebView)         │
│ ツール、レイヤー、プロパティ、IME、データ表     │
│ 作品画像の表示／選択枠のみ。最終組版をしない    │
└─────────────────┬────────────────────────────┘
       型付きコマンド│   ▲ 読取モデル・進捗・画像buffer
┌─────────────────▼────────────────────────────┐
│ Rust Host                                    │
│ 文書の正本、編集トランザクション、Undo/Redo     │
│ ファイル許可broker、保存、ジョブ管理            │
└─────────────┬────────────────────┬───────────┘
       固定snapshot / bytes       │ 許可済み書込
┌─────────────▼────────────────┐   ▼
│ Native Render Worker         │  Project package / Jobs
│ Decode → Bind → Layout       │  SQLite index / Recovery
│ → Shape → Preflight          │
│ → RenderPlan → Skia → Encode │
└──────────────────────────────┘
```

別プロセスのworkerは主にクラッシュ・停止の影響を分離する。プロセスを分けただけで秘密情報へのアクセスや通信が禁止されるわけではない。実際の境界は、OSのサンドボックス、Hostの許可検査、渡すデータの最小化で成立させる。

workerは固定の同梱バイナリーだけをHostが起動する。ユーザーに任意コマンド実行APIを公開しない。通信用にTCPサーバーを立てず、制限されたstdio pipe等を使用する。長さ上限を持つ型付きメッセージと、サイズ検査されたバイナリーを扱う。

## 3. 主要モジュール

| モジュール | 責務 | 禁止する責務 |
|---|---|---|
| design-model | 文書型、ID、版、意味検証、移行 | UI状態、OSファイル操作 |
| design-edit | 編集command、transaction、Undo/Redo、read model | Skiaで直接描くこと、任意I/O |
| design-layout | スロットbinding、variant、制約、fit、geometry | 勝手なアートディレクション |
| design-text | フォント解決、shaping、段落、禁則、hit情報 | OS任せの無断fallback |
| design-render | RenderPlan、Skia描画、効果、色変換 | 文書を破壊する更新 |
| design-export | PNG/JPEG/WebP/SVG/PDFと機能適合検査 | 未対応効果の黙殺 |
| design-preflight | 判定と原因、node/row/variantの対応 | 問題箇所の無断修正 |
| project-store | package、原本素材、checksum、移行、復旧 | クラウド保存 |
| job-runner | snapshot、進捗、再開、成果物manifest | 実行中に入力原本を読み直すこと |
| desktop-host | OS対話、許可broker、IPC、worker監督 | 広範なJS向けファイルAPI |

最初から全てを別サービスにしない。Rustのworkspace内で依存方向を分ける。外部サーバー、メッセージブローカー、分散基盤は不要だ。

## 4. 技術選定

| 層 | 本線の選定 | 理由・検証条件 |
|---|---|---|
| デスクトップshell | Tauri 2 | ローカル資源同梱、Rustとの連携、能力制限。実機で検証 |
| UI | React＋TypeScript strict＋Vite | 制作UIに集中。Nodeは開発時のみ |
| 正本・command | Rust | 意味検証と編集を一か所へ集約 |
| 描画 | skia-safe経由のSkia | 共通2D描画経路。CPU基準経路を先に確立 |
| 文字 | SkParagraph等＋HarfBuzz／Unicode支援 | buildに含まれる実装を確認。禁則・fitは製品側で検証 |
| 入出力 | Rustの検証済みcodec／Skiaのcodec | 不要なNode sidecarを持たない |
| 永続化 | versioned JSON＋content-addressed files | 可搬性と復旧。独自packageが正本 |
| 補助台帳 | SQLite | jobs、最近使った項目、cache index、復旧索引。外部DB不要 |
| 試験 | Rust unit/integration/property＋UI unit/E2E | ブラウザー試験とnative実機試験を分離 |

Sharp／libvipsとSkiaを最初から二重の画像描画基盤として採用しない。Rust＋TauriへNode sidecarを増設する理由が現時点ではない。HarfBuzzも独立した別文字エンジンとして並走させず、使用するSkia text stackを調べて不足部分を補う。

正確な依存version、feature flags、Skia revision、codec・Unicodeデータ版はPhase 0で互換性を確認してlockする。「常にlatestを使う」という指定はしない。rust-skiaのnative buildは事前取得やnative依存を伴うため、開発時の準備と完成品のオフライン動作を区別する。[S03]

## 5. 描画の一本道

```text
DesignDocument + Dataset row + LayoutVariant + Asset/Font snapshot
  → 構文・参照検証
  → 許可済みslotだけbinding
  → variantを解決
  → geometry / image cropを計算
  → font解決・文字形成・改行・fit
  → 品質検査とexport機能適合検査
  → immutable ResolvedDesign + RenderPlan
  → 共通描画コマンド
  → Preview buffer / Raster / Vector exporter
```

BindやLayoutをUI側とworker側へそれぞれ実装しない。RenderPlanは既に座標・字形・画像変換が解決されたものとし、描画中にテキストを再測定しない。glyph位置、clip、effect順序、alpha、色プロファイルは計画に固定する。

プレビューとラスタ出力は同じSkia build・同じCPU基準経路を共有する。SVG／PDFは同じResolvedDesignを使用するが、別の出力backendであるためピクセル一致を無条件に保証しない。変換ごとの適合検査・再描画比較を行う。

## 6. 高精度プレビューと応答性

ユーザーの操作直後は、選択枠・ガイド・カーソル等をUIで即時更新する。直後に最新revisionの実描画を反映する。低解像度previewや操作中の代理表示を使う場合は、最終品質であるかをUIで区別する。

React側で作品を別レイアウトとして描き直さない。ラスターviewportはnative bufferを表示するだけにする。作品の下絵と、編集用オーバーレイを分離する。

転送はJSON化した巨大な配列やbase64 PNGを毎フレーム送る方式を避け、raw byte IPC、dirty tile、cacheを検証する。Tauriはraw responseのArrayBuffer等を扱えるが、転送・変換・表示の全コストを実測する。[S08]

各描画要求にdocumentRevision、viewport、requestIdを付ける。古い要求は中止または結果破棄し、新しい編集結果を古いフレームで上書きしない。静的レイヤーの表示リスト、decode済み画像、組版結果、タイルをhashでcacheする。

ドラッグ中は複数preview要求をまとめる。操作確定時に1件のedit transactionとして記録し、Undoも1回で戻る。UIのoptimistic表示は正本ではなく、Hostから失敗が返れば復元する。

CPU基準経路の速度が不足する場合は、まずdirty rectangle・tile・解像度・cacheを改善する。その後GPU previewを検討するが、最終CPU出力との画質差を検査し、正確プレビュー機能を残す。別実装への変更はADRとゲート再試験を必要とする。

## 7. テキストとIMEの責務

WebViewの入力欄はIMEからテキストを受け取るためのインターフェースだ。画面内の文字の最終メトリクスには利用しない。

流れは、composition start→未確定入力→composition end→確定編集transaction→native shaping→nativeからcaret／selection boundsを返す、となる。未確定入力を何度も文書に追加したり、変換途中の文字でバッチを開始したりしない。

JavaScriptのUTF-16 index、RustのUTF-8 byte offset、ユーザーが認識する書記素、glyph clusterを同一視しない。offset変換は共通テキスト層で扱い、サロゲート対・結合文字・濁点・絵文字等の境界をテストする。v1で描画未対応の文字でも入力とエラー表示でデータを壊さない。

HarfBuzzは文字形成の役割を担い、段落折り返し、全ての禁則、文字あふれの製品方針まで担当するわけではない。SkParagraphの挙動とUnicode境界を確認し、必要な日本語ルールだけを明示的に追加する。全文字列を1文字ずつ単純加算して行幅を求めない。[S04][S05]

## 8. 画像・色・画質

画像の取り込みでは、内容識別→寸法上限→decode→EXIF orientation→ICCの解釈／色変換→正規化→hash登録の順とする。元画像も保持し、出力メタデータへの位置情報等の転写はしない。

v1の色基準はsRGB。指定色はsRGBとして保存し、表示buffer・出力画像には対応する色情報を与える。色タグがない画像はユーザーに明示した規則で扱う。モニターやPDF viewerが異なると画面の見え方まで一致するとは限らない。[S07]

内部合成の色空間・surface精度・alpha方式をRenderProfileに明記する。Phase 0では高精度surfaceと8bit surfaceのグラデーション・透明縁・ぼかし・速度を比較し、合格したプロファイルを固定する。数値精度の高さとlinear-light合成は別の設定であり混同しない。

出力は最終寸法へ直接レンダリングし、小さいpreviewを拡大して納品画像にしない。固定したsamplingを使い、透明な写真の縁、細線、斜線、小さい文字、グラデーションを検査する。任意のシャープ化を勝手に加えない。

## 9. 資源使用とバッチ

UI用のpreviewを優先し、バッチは制限付き並列実行にする。初期の最大同時実行は2を上限とし、資源予約に応じて1へ落とす。CPUコア数分を無条件に並列化しない。

1件のメモリー予測には、画像decode、出力surface、効果の一時surface、clip、色変換、encode bufferを含める。出力幅×高さ×4だけでは見積もらない。

BatchSnapshotは文書・データ・variant・素材・フォント・RenderProfile・encoder設定を固定する。ジョブ開始後に編集を続けても、実行中のsnapshotは変えない。外部画像の原本が更新された場合も、実行中ジョブへ混入させない。

出力はtemporary file→検証→同一filesystemでcommitの順とする。キャンセルやクラッシュ時のpartial fileを完成品名で残さない。再開時は予定したinput digestと既存出力のchecksumを照合し、ファイルが存在するというだけでskipしない。

## 10. 保存・復旧

編集時はアプリ専用のローカル作業領域を使用し、保存時に独自packageを新しく作ってから原子的に置き換える。ZIPへのインプレース追記を通常の保存経路にしない。

文書の正本は現在のRust sessionと保存package内のJSON。SQLiteは索引・ジョブ状態・復旧参照を支えるが、SQLiteが壊れたときに作品を失う唯一の保存先にしない。ジョブsnapshotと出力manifestはファイル側から再検査・再構築可能にする。

自動復旧は確定編集のcheckpoint＋連番journalを使用し、最後の完全なrecordまで復元する。チェックサム不一致や途中切断は末尾の破損として検出する。既存の正常packageは、復旧に成功するまで上書きしない。

保存失敗、disk full、権限不足、worker crash、OS終了、同名ファイル競合を試験する。通常のUndo履歴をアプリ再起動後まで維持することはv1の必須ではない。保存済み文書・確定編集の復旧と混同しない。

## 11. 出力形式ごとの責務

PNG／JPEG／WebPは共通CPU rasterが品質基準。SVGは独自serializerで対応サブセットだけを書き、未対応効果を検査する。SVGの文字をoutline化するモードは外観優先であり、文字編集性を失う。外部画像参照は使わない。

PDFもResolvedDesignを入力とする。最初はRGB校正用。font embeddingを行うモードとoutlineモードは区別し、どちらも利用条件と表示結果を確認する。効果の互換性を検査し、必要な場合はユーザー承認のうえで依存を含むgroup／pageをraster化する。効果の一部だけを画像にして背景とのblendを壊さない。[S06]

いずれもnative documentの代わりにはならない。完全な編集再開には独自packageを使う。

## 12. 再現性の定義

同じ入力とは、文書revision、dataset行、variant、assetのbytes、fontのbytesとface、エンジンbuild、Unicodeデータ、色・sampling・AA・encoder設定まで一致することを指す。

同一の検証済みCPU環境では、正規化したraster出力bytesの一致を目標とする。日時や不定順序のmetadataは含めない。GPUや異なるCPU architecture／OS／codec版をまたぐ無条件のbyte一致は約束しない。

異なる対応環境では、改行位置・ノード座標・glyph選択・crop等の構造的一致と、合意した画像差分基準を別に検査する。フォントが同じ名称でもファイルhashが異なるなら同一入力ではない。

## 13. 実装ディレクトリー案

```text
apps/desktop/ui/
apps/desktop/src-tauri/
crates/design-model/
crates/design-edit/
crates/design-layout/
crates/design-text/
crates/design-render/
crates/design-export/
crates/design-preflight/
crates/project-store/
crates/job-runner/
crates/render-worker/
packages/contracts-generated/
fixtures/public-safe/
tests/golden/
tests/offline/
tests/security/
docs/adr/
```

これは目標の責務分割だ。初日に空ディレクトリーと未実装関数だけを大量生成する必要はない。最初の縦切り実装に必要なcrateから追加する。
