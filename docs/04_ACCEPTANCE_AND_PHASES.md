# 04 — 受け入れ試験と開発順序

## 1. 検証状態

このパッケージを作成した時点では、製品アプリをビルド・実行していない。以下は合格基準の提案だ。実測値や達成済みの数値ではない。

技術選定、画質、nativeプレビュー転送、日本語IME、オフライン初回起動が最大のゲートになる。機能数でこれらの未検証を埋め合わせない。

## 2. 標準fixtureと性能目標

仮の標準実機はApple Silicon M1・16GB・ローカルSSD。OS版、画面scale、build、CPU profile、電源状態、codec設定を記録する。実際の検証端末が異なる場合はその実機を明記し、同じ成績とみなさない。

標準fixtureは、1,080×1,350px、120レイヤー、8枚の画像（各12MP以下）、日本語＋英語、フォント3face、画像clip、2種のgradient、影を含む。比較用に小さいfixtureと重いstress fixtureを分ける。

| 指標 | 目標案 | 計測の注意 |
|---|---|---|
| 起動→新規編集可能 | p95で5秒以内 | release build、初回と2回目以降を別記 |
| 選択・ハンドル等の操作反応 | p95で50ms以内 | UI反応と正確な作品再描画を分ける |
| 通常変更→正確preview | warm p95で250ms以内 | 実作品のnative描画・転送・表示まで |
| 標準PNGの1件出力 | warm p95で1秒以内 | 事前decode済みかを明記 |
| 100行×3variantのPNG量産 | 180秒以内 | 固定dataset／素材、UIは操作可能 |
| 標準fixtureの総メモリー | 2GiB以内 | Host・WebView・workerの合計で計測 |
| 自動復旧 | 確定編集から2秒以内のcheckpointを目標 | IME変換中入力は保存済み扱いにしない |

平均だけでなくp50／p95、cold／warm、最大メモリー、失敗件数を報告する。出力に重い効果や大きな画像を足した場合は別fixtureとして測る。目標未達なら原因を分離して改善し、数値を黙って緩和しない。

## 3. 画質の合格基準

基準画像は作者が目視承認したローカルfixtureから作る。自動生成した最初の画像を無条件に正解画像へ採用しない。

同一build・同一CPU環境では、同じinput digestから10回出力し、正規化したPNG等の出力hashが一致することを目標とする。codec metadataや時間依存要素を除去した設定を固定する。

正確previewとPNGの比較は、同一の出力解像度・色空間・AA・samplingで生成したbufferを比較する。ズームされた画面のスクリーンショットと原寸PNGを直接byte比較しない。

ベクター出力を再描画した際の差分は別指標にする。書体、改行、glyph、座標、clip範囲、gradient、画像配置が一致することを先に確認し、許容するraster差分をfixtureごとに設定する。SSIMだけで文字抜け等を見逃さない。

精度の基準案は文書座標で0.01du以内、画像の書き出し寸法は厳密一致。金銭・注意事項・ロゴの欠落は1文字・1要素でも不合格。輪郭の数値誤差と内容の欠落を同じ許容差に入れない。

## 4. 主要な受け入れ試験

| Test ID | 対象要件 | 試験と合格条件 |
|---|---|---|
| T-OFF-01 | OFF-001 | クリーンなrelease配置からoffline初回起動→制作→保存→再読込→量産→書出を完了 |
| T-OFF-02 | OFF-002 | online状態で同操作を行い、app＋workerの外向き通信／DNS／接続試行／待受0 |
| T-AI-01 | AI-001 | runtime依存、同梱資源、プロセス、call pathにAI SDK・model・inferenceがない |
| T-SEC-01 | SEC-001 | 選択していないフォルダーへのread/writeが拒否され、既存ファイルに変化がない |
| T-SEC-02 | AST-001,AST-002 | 危険SVG、壊れた画像、展開爆弾、過大寸法、外部参照を拒否し、通信・UI crashなし |
| T-SEC-03 | SEC-001,OFF-001,OFF-002 | リモートmountやクラウドplaceholderを拒否し、入力取込で外部取得を要求しない |
| T-DOC-01 | DOC-001,DOC-002,DOC-003 | 保存→再読込で文書・素材参照・node順序・描画結果を保持 |
| T-EDT-01 | EDT-001,EDT-002,EDT-003 | 選択・座標・パス・clip・gradient等の操作が実描画へ反映し保存できる |
| T-UNDO-01 | EDT-004 | edit→undo→redoで意味上同一の文書状態へ戻り、batch中でも破壊しない |
| T-IME-01 | TXT-001 | 日本語変換・再変換・取消・結合文字・英日混在で二重挿入なし |
| T-TXT-01 | TXT-002,TXT-003 | 固定フォント、禁則、最大行数、手動改行のfixtureで想定通り |
| T-TXT-02 | TXT-004,FNT-001 | 字形やフォント欠落は該当nodeを示して停止し、system fallbackを使わない |
| T-TXT-03 | TXT-005,DES-001 | allowlist内のfitだけが適用され、最低サイズ未満への変更は拒否 |
| T-AST-01 | AST-003,AST-004 | EXIF orientation、ICC、contain/cover、focal point、alphaのfixtureを通過 |
| T-BRD-01 | BRD-001,LOG-001,FNT-002 | ロゴ比率・最低幅・余白・許可asset・font同梱条件を検査 |
| T-TPL-01 | TPL-001,TPL-002,TPL-003 | datasetから未公開属性の変更不可、版変更は新revisionに限定 |
| T-VAR-01 | VAR-001,VAR-002 | 承認済み3variantを生成し、未知ratio／未定義variantを拒否 |
| T-BAT-01 | BAT-001,BAT-002 | 先頭ゼロ、空欄、重複列、不正encoding、型不一致を検出して全件数一致 |
| T-BAT-02 | BAT-003,BAT-004 | 実行中に元文書／素材を変更してもsnapshot出力は不変。中断から再開可能 |
| T-BAT-03 | BAT-005 | 同名、危険な出力名、既存出力のchecksum不一致で無断上書きなし |
| T-EXP-01 | EXP-001 | PNG/JPEG/WebPの寸法・alpha・色タグ・背景色・品質設定が仕様通り |
| T-EXP-02 | EXP-002,EXP-003 | SVG/PDFは機能適合検査を通り、raster化・outlineを報告。未承認の変換なし |
| T-PRE-01 | PRE-001 | ERRORで出力停止し、row/node/variant/reason/remedyを表示 |
| T-REP-01 | REP-001 | 同一の検証済みbuild／環境で10回のraster出力hashが一致 |
| T-REC-01 | DOC-003 | disk full、保存途中停止、journal末尾破損、worker crash後に正常文書を復旧 |
| T-PERF-01 | REP-001 | 標準fixtureの計測条件を固定し、性能表を実測値と未達項目付きで報告 |

## 5. 試験層

Rustのunit testで制約、binding、型、パス、filename、row検証等を行う。property testで乱れた入力・undo・範囲・循環検出を調べる。画像比較・golden testはnative engineの生成物に対して行う。

UIのunit／browser E2Eは入力と表示状態の確認に使う。ただしTauri IPC、OS file picker、IME、native worker、release権限まで合格した証明にはならない。

macOSのnative E2Eは、現行Tauri資料のWebdriverIO embedded方式等を候補に実証する。通常のtauri-driver直結はmacOSと同じ扱いではない。試験用のembedded driver／test serverはtest buildだけに限定し、製品releaseへ同梱・有効化しない。実機操作やXCTest系の補助を含め、確認できた経路だけを報告する。[S10]

オフライン・セキュリティ試験は配布するreleaseそのものに行う。dev serverやテスト専用IPCがある状態の結果を代用しない。通信の試験は全通信のpacket captureだけでなく、process attributionと接続試行の観測を併用する。

## 6. 開発フェーズ

### Phase 0 — 技術検証。製品の広いUIを作る前に実施

確認事項：Skia build、文字stack、ローカルfont、native buffer→WebView、IME、日本語改行、safe SVG、色、透過、sandbox、offline release起動。

成果物：1つの中核文書を読み、画像・日本語文字・outlineロゴ・gradient・clipを描画し、PNGを出す小さな実装。fontと素材は安全なローカルfixtureをユーザーが用意する。試験用fixtureとユーザーの本番デザインは区別する。

ゲートG0：実機で表示・出力と問題検出が確認でき、native描画転送の速度・文字のリスク・依存buildが測定されている。未達なら原因と代替案をADRにし、機能を増やさない。

### Phase 1 — 正確な制作エンジンと保存

文書schema、編集command、画像・文字・図形、binding、単一variant、preflight、PNG/JPEG/WebP、保存・再読込・復旧を実装する。

ゲートG1：同一文書が再描画でき、フォント欠落・overflow・参照切れで止まる。実データを使わないfixtureで繰り返し出力が一致する。

### Phase 2 — 独自デザインエディター

レイヤー、数値編集、整列、スナップ、ベジェ編集、clip、効果、文字スタイル、IME、Undo/Redo、ブランド、templateロックを統合する。

ゲートG2：外部Illustratorに頼らず、アプリ内で1つの完成したデザインを作成し、編集・保存・復帰できる。未実装ボタンがなく、実際の操作を記録した試験がある。

### Phase 3 — 量産の実用化

CSV/JSON、画像対応、全件検査、複数variant、一覧確認、固定snapshot、queue、cancel、resume、出力名、manifestを実装する。

ゲートG3：30件×3variant等の固定fixtureで件数が合い、失敗が見え、再開で欠落や上書きを起こさない。速度を測る。

### Phase 4 — v1リリース品質

SVG交換、RGB校正PDF、ライセンス管理、safe import、OS権限、コード署名、offline installer、release監査、性能・画像の回帰試験を完了する。

ゲートG4：01のv1必須要件、03の禁止事項、04の該当試験が全て合格。例外が残る場合はv1完成と呼ばず、preview版として対象を明記する。

### 後続リリース — 拡張

Windows／Intel、縦組み・ルビ・パス上文字、可変フォント、より高度なvector編集、商業印刷を個別ゲートで追加する。実行時AIと外部通信は禁止のままであり、後続フェーズの候補に入れない。

## 7. 完了の報告形式

各ゲートで、実装範囲、実機・OS・build、実行した試験、成功／失敗／未実施、fixtureのhash、実際の生成物、制限、次の小さな作業を残す。

「テストコードを作った」と「テストが成功した」、「画面にボタンがある」と「実処理が動く」、「オフライン用設定」と「releaseが通信しない」は別物だ。未実施を成功に含めない。
