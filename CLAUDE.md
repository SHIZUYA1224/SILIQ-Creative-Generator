# SILIQ Creative Generator — Project Instructions v0.2

## Mission

独自デザインエディターと、承認済みデザインの量産エンジンを作る。デザインの判断はユーザーが行い、アプリはその指定を正確に再現する。

## Non-negotiable

- 完成品は完全オフライン。初回起動・制作・保存・復旧・書き出しに通信不要。
- runtime AI／LLM／ML推論、model weights、AI SDK、外部API、認証、cloud DB、telemetry、update check、remote font／asset取得は禁止。
- Claude Codeは開発時のみ。製品から起動せず、認証情報や会話を同梱しない。
- 顧客素材・業務資料・実案件CSVを外部へ送る許可はない。安全なfixtureだけで開発する。
- ユーザーの指定外の色・書体・ロゴ・レイアウト変更は禁止。自動fitも宣言された範囲内だけ。
- 未対応や不足fontを黙って置換・省略しない。errorにして止める。
- 広範なfilesystem access、任意shell、任意script／plugin実行、sandbox無効化、OS保護の全体無効化は禁止。

## Read first

`docs/01_REQUIREMENTS.md`
`docs/02_ARCHITECTURE.md`
`docs/03_SECURITY_AND_OFFLINE.md`
`docs/04_ACCEPTANCE_AND_PHASES.md`
`docs/05_DATA_MODEL_AND_CONTRACTS.md`
`docs/06_DECISIONS_AND_RISKS.md`
`schemas/design.core.schema.json`
`examples/design.core.sample.json`

## Architecture

Tauri 2＋React／TypeScript UI。Rustが文書・編集・権限の正本。Skiaを共通描画の本線とし、Phase 0で実機成立性を検証する。CPU基準出力を先に作る。ブラウザーDOM、Fabric／Konvaの内部JSON、SVGそのものを作品の唯一の正本にしない。

文字形成と段落・禁則・fitを混同しない。SkParagraph／HarfBuzz等の実際のbuild構成を確認し、文字layoutをUIとnativeで二重実装しない。Sharp／Node sidecarや別レンダラーを安易に増やさない。

SVG／PDFは派生形式。対応外の効果、raster化、outline化、font embedding、編集性の変化を明示する。v1はsRGB静止画とRGB校正PDF。AI／PSD互換、商業印刷、PDF/X完全対応を未検証のまま主張しない。

## Working method

既存repoを読む。勝手に初期化・上書き・force pushしない。元スターターがある場合は仕様差分を先に報告する。秘密・顧客素材のあるフォルダーを探索対象にしない。

最初はPhase 0の小さな実機spikeだけを計画・実装する。広い未完成UIを先に作らない。実機で未検証のengineや権限設定を確定扱いにしない。

依存version・native revision・feature flags・licenseを確認し、lockする。存在しないcommandをあるものとして使わない。導入済み環境を確認してから実際のbuild／test commandをREADMEへ記録する。

スキーマは厳格にし、意味検証もRustで実施する。sampleは中核subsetかつ素材未同梱である。実ファイルなしに正常描画したと主張しない。

編集はtransactionとrevisionで管理し、Undo／Redo・IME・古いpreview破棄を試験する。バッチは固定snapshotを使い、全件検査と部分失敗を可視化する。

ファイルはgrantで制限し、入力を未信頼として扱う。CSPだけでRustの通信を防げるとは考えない。capabilityとcustom command露出を監査する。

テストdriver／dev server等はtest／development buildだけ。releaseへ同梱・有効化しない。browser E2Eをnative releaseの検証と混同しない。

各sliceの最後に、変更範囲、実行した試験、実機／build、生成物、失敗、未実施、制限、次の小さな作業を報告する。スクリーンショットだけでなく実ファイルを検査する。

## First delivery

画像＋ローカルの日本語font＋outline logo＋gradient＋clipを1つの中核文書から描き、native previewとPNG出力を確認する。欠けたfont／overflow／安全でないSVGで止まり、同条件の反復出力とoffline release起動を試験する。資源・転送・組版の結果をG0 reportへ記録する。

## Approval boundaries

依存の大幅変更、OS権限追加、広いfilesystem変更、破壊的操作、配布・公開、実案件素材利用は明示承認が必要。runtime AI／外部通信禁止を「便利だから」で解除しない。

このファイルは行動指針であり強制sandboxではない。禁止条件は設定・依存監査・テスト・reviewでも検証する。
