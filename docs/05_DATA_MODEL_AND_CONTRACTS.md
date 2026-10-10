# 05 — データモデルと処理契約

## 1. デザイン・素材・データ・ジョブを混ぜない

| モデル | 保存するもの | 例 |
|---|---|---|
| DesignDocument | レイヤー、文字スタイル、geometry、slot、variant、制約 | 見出しを置く領域と書体 |
| BrandSnapshot | 確定した色・スタイル・ロゴの版 | 使用可能なロゴasset ID一覧 |
| AssetCatalog | 画像／ロゴの内容hashとmetadata、元データ | 画像の向き・色プロファイル |
| FontManifest | hash、face、features、許可したfallback、同梱方針 | フォント名だけに依存しない指定 |
| Dataset | 各行のtyped valueとasset ID | 店舗名、価格文字列、店舗写真 |
| ExportRecipe | variant、形式、寸法、色、背景、品質、命名 | PNG・1080×1350 |
| BatchSnapshot | 上記の固定revisionと必要なbytesの参照 | 開始後に原本変更の影響を受けない入力 |
| OutputManifest | 成否、hash、適用fit、変換、ファイル名 | 生成結果の照合・再開 |

同じテンプレートに異なるDatasetを適用することと、同じ画像に違う勝手なデザインを作ることは別だ。bindingは指定されたpropertyにしか作用しない。

## 2. ネイティブ保存形式

拡張子は暫定で`.siliqcg`とする。ファイル形式の名称はこのプロジェクト内の提案であり、登録済みの標準形式ではない。

```text
campaign.siliqcg  (ZIPコンテナ。インプレース編集しない)
  manifest.json
  design.json
  brand.json
  assets/<content-hash>.<ext>
  fonts/manifest.json
  fonts/<content-hash>.<ext>    # 同梱を許可したfontだけ
  previews/thumbnail.png       # 派生物。正本ではない
```

manifestにはformatVersion、schemaVersion、最低対応engine version、document ID、revision、entryのhashとsizeを保存する。format versionとrender engine revisionは別に管理する。

フォントは既定で参照型とし、hashとfaceを保存する。別端末で同じファイルを解決できなければE_FONT_MISSING。許可した場合だけbytesをpackageに入れる。元PCの絶対パスを共有用packageに埋めない。

document migrationは連続したversion間の純粋変換として実装し、元ファイルのコピーを残す。新しい機能を理解できない古いアプリが無視して再保存することは禁止する。

## 3. Scene Graph

v1のノード型はartboard、group、shape、path、text、image、logo／imported-vectorを基本とする。frame、clip、mask等は型または参照として明示し、参照可能な型を制限する。

各nodeは安定ID、親、描画順、ローカルgeometry、transform、visible、locked、opacity、style/effectを持つ。子の描画順は配列で固定し、mapの反復順やhash順へ依存させない。

UIの選択状態、ズーム、開いているペイン、caretの点滅は作品モデルに保存しない。これらはローカルUI stateとして分離する。

座標はアートボード左上原点、右が正のX、下が正のY。単位duを使い、raster 1x時に1du＝1pixelと定義する。浮動小数は有限値のみ許可し、寸法上限を持つ。出力ピクセル数は整数とする。

変形順はlocal bounds→local affine transform→parent transform→artboard→export scale。roundingは最後のrasterizationまで行わない。SVG変換等では同じ座標規約へ正規化する。

## 4. Typed Slot

slotは任意JSON pathやJavaScript式ではない。text、image、logo-choice、限定されたstyle-choice等の有限型と、許可したtargetの組として定義する。v1の最初のsliceはtextとimageに絞る。

例：headline slotはheadline-nodeのtext.valueへだけ適用可能。headlineからx/y/color/fontを変更することはできない。logo-choiceはbrand snapshotの許可assetだけを参照する。文章中の記号がコードとして解釈されることはない。

必須、文字数上限、空欄方針、画像種別、許可asset、入力表示名を持つ。optionalの空欄を隠す動作はユーザーが事前に指定した場合だけ有効にする。required欠落は自動生成せず停止する。

## 5. Variantと制約

variantにはID、アートボード寸法、作者の承認版、node override、許可したfit方針を保存する。override可能なfieldはschemaで限定する。

同じheadline slotを、1:1では右側、4:5では下部に配置する等の明示的な設計を保存する。variantが違う場合にfont sizeが違うのは作者の指示であり、AIによる変更ではない。

constraintは親アンカー・固定値・割合・最小／最大等の少数の型に限定する。参照graphはDAGとして検証し、循環・未知参照・矛盾をerrorとする。v1に汎用の任意コード実行は持ち込まない。

## 6. 文字の保存

TextNodeには原文、language、direction、text runs、font ID、size、letterSpacing、lineHeight、paragraph alignment、OpenType feature設定、text box、overflow policyを持つ。

fit候補は作者が承認した降順のsize配列等として保存する。candidateを走査してlayoutを測り、最初に制約を満たしたものを採る。候補の変更はtemplate revision変更となる。

fallbackを許可する場合は作者が指定したfont manifestの順序だけを使い、実際に使ったfontをOutputManifestへ残す。既定はfallback禁止。擬似bold／擬似italicも無断では適用しない。

解決後のglyph run、行境界、ink bounds、caret mappingはDerived Layoutとしてcacheできるが、元テキストの代わりとしてだけ保存しない。cache keyにはfontのhash・設定とengine revisionを含める。

## 7. 画像の保存

ImageNodeはasset ID、fit、focal point、clip、transform、調整設定を持つ。ロゴは基本containで縦横比を維持する。画像の縦横比を潰すstretchは、作者が明示選択した場合だけ使用可能にする。

coverの拡大率は枠幅／画像幅と枠高／画像高の大きい方、containは小さい方から求める。focal pointは0〜1の正規化座標で保持する。crop位置は画像と枠の境界にclampする。ユーザーが設定した保護領域が欠けたら検査エラーとする。

保護領域は手動指定。何が人物で何が商品かを判別するモデルは使わない。画質検査は表示寸法と元画素数等の客観的な条件だけを扱う。

## 8. IPC契約

Hostが文書の正本を持つ。UIからのmutationにはsession ID、expected revision、transaction ID、typed commandを付ける。revisionが古い場合はE_REVISION_CONFLICTとしてread modelを再取得する。

代表的なcommandの設計案：

| command | 入力 | 結果 |
|---|---|---|
| open_project | OS選択で取得したgrant ID | session ID、read model、issues |
| apply_edit | session、expectedRevision、command | newRevision、差分read model |
| import_asset | 読取grant ID、取込方針 | asset ID、検査結果 |
| request_preview | session、revision、viewport、requestId | render ticket、frame metadata |
| run_preflight | snapshot reference、recipe | node/row/variant単位のissues |
| start_batch | 固定snapshot、recipe、出力grant | job ID |
| cancel_batch | job ID | 更新した状態 |
| save_project | session、expectedRevision、保存grant | 保存revision、checksum |

全commandはRust側でも型・size・所有session・grantを再検証する。TypeScriptの型が通ることは、native側で安全な入力だという保証にはならない。

frame headerはdocumentRevision、requestId、width、height、stride、pixelFormat、alphaType、colorSpace、quality、payloadLengthを含む。標準の転送profileはRGBA8／sRGB／straight alphaとする案をまず検証する。Skia内部のpremultiplied alphaから変換するなら一度だけ行い、WebView側の期待するpixel形式と一致させる。二重premultiplyやチャンネル順違いは透明な縁の試験で検出する。

frame payloadのsizeをwidth×strideと照合し、古いrevisionを表示しない。巨大bufferや不一致headerを拒否する。Base64文字列を画面更新の標準経路にしない。

## 9. エラーの統一形式

すべてのissueはseverity、code、message、location、remedyを持つ。locationはdocument／node／row／variant／export formatを指せるものとする。詳細診断と、機密情報を含めない通常ログを分ける。

主要コード：E_FONT_MISSING、E_GLYPH_MISSING、E_TEXT_OVERFLOW、E_ASSET_MISSING、E_ASSET_TYPE、E_SVG_UNSUPPORTED、E_EXTERNAL_REFERENCE、E_LAYOUT_UNDEFINED、E_CONSTRAINT_CYCLE、E_LOGO_CLEARSPACE、E_EXPORT_UNSUPPORTED、E_OUTPUT_COLLISION、E_PATH_DENIED、E_RESOURCE_LIMIT、E_SAVE_FAILED、E_REVISION_CONFLICT。

warningは作者が定めた最低画質未満等の注意。文字欠落や安全違反をwarningへ格下げしない。warningを認めて出力した場合もmanifestに残す。

## 10. バッチ状態と再実行

jobの状態はqueued→preflighting→ready→running→completedを正常系とする。途中にblocked、cancelling、cancelled、failed、interrupted、completed_with_errorsを定義する。

itemはrow ID×variant ID×formatから安定したキーを生成する。start時にinput digestを計算し、実行成功時にoutput digestと変換報告を保存する。再開は同じinput digestのjobに限る。入力が違う場合は新jobにする。

命名には限定されたplaceholderだけを使う。任意テンプレート式は評価しない。出力先はdirectory grantの中に限定する。名前の衝突や禁止文字はプレビューで示し、無断で元のidentifierを書き換えない。新しい出力directoryを既定とし、既存ファイルの上書きは明示操作を必要とする。

## 11. Schema運用

`schemas/design.core.schema.json`はPhase 0で扱う厳格な中核subset。group、rect、text、image、logo、variant、typed slot等を持つ。実装開始時にRust型との対応を作り、JSON Schema／TypeScriptの生成経路を1つに定める。スキーマとコードを別々に手で増やして不一致を放置しない。

schema検証だけでは参照整合・graph循環・font実在・clipの意味・ライセンス・画質を検証できない。意味検証とpreflightが必要だ。

同梱sampleには実画像と実フォントのbytesがない。`unresolved`の参照を意図的に含める。構造検証には通るが、そのまま書き出し可能であるという意味ではない。実装ではE_ASSET_MISSING／E_FONT_MISSINGで停止することを期待する。
