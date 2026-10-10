# SILIQ Creative Generator

広告・SNS画像と、ブログ記事の図解・比較表・説明画像を作るローカルデザインアプリです。人がレイアウトと内容を決め、同じネイティブ描画エンジンで編集結果を確認し、保存・検査・書き出し・CSV／JSON量産を行います。

**Mac向けデスクトップアプリを実装済みです。** SwiftUI／AppKitの画面とRust／Skia CPUの描画を組み合わせています。WebViewを初期化しない構成です。製品実行時のAI・外部API・クラウド・認証・テレメトリーは使わない構成で、素材と作品は端末内で扱います。開発用の依存取得には通信が必要です。

## できること

- 広告、商品紹介、記事の要点、比較表の同梱テンプレートから制作。白紙の新規作品も作成できます。
- テキスト、画像、図形、パス、グループ、レイヤー、色、線、グラデーション、影・ぼかしを編集。サイズ別アートボード、ブランド色・文字スタイル・ロゴ制約、Undo／Redoに対応します。
- PNG／JPEG／WebP画像、安全なSVG、フォントを端末から登録。未対応SVG要素、スクリプト、外部参照、不正な内容は拒否します。
- `.siliqcg`作品の保存・読み込みと、編集中の復旧。フォントは埋め込み許可を明示したものだけ作品に同梱します。不足時は**元と同じフォントファイル**を再登録し、別フォントへ自動置換しません。
- 承認したアートボードをPNG／JPEG／ロスレスWebP／SVG／PDFで書き出し。CSV／JSONの列を差し込みスロットへ対応付け、全候補の検査と縮小画像一覧を確認してから量産できます。
- 量産の進捗、キャンセル、検証付き再開に対応。再起動後の再開では元の出力フォルダーを選び直します。設定画面で保存領域を確認し、使用中の作品・ジョブ・最新復旧を保護して履歴を整理できます。

## 基本の使い方

1. テンプレートを開くか、新規作品を作り、「制作」モードで編集します。
2. 文字欠け、オーバーフロー、素材不足、ロゴ制約などの検査結果を直し、各アートボードを「承認する」で確定します。制作上の変更後は再承認が必要です。
3. 保存または書き出しを行います。「量産」モードでは作者が宣言したスロットの文字・画像差し替えだけを許可し、レイアウトを保護します。
4. CSV／JSON量産では一意の行ID、列とスロットの対応、承認済みサイズ、形式、出力名、保存先を指定します。既定は全件のエラーが解消するまで停止。「正常な候補のみ」の出力は明示選択が必要です。

JSONは同じ列を持つオブジェクトの配列で、値はすべて文字列にします。例：`[{"id":"001","price":"0009"}]`。数値・重複キー・列の不一致を拒否し、先頭ゼロを保持します。どちらの形式も数式を実行しません。入力は20 MiB・10,000行までです。JSON内の文章の改行はLFを使い、自動変換が必要なCR／CRLFは拒否します。

量産中は検査時の作品・データ・素材・出力設定を固定します。候補一覧の画像は共通描画から作る「検査用縮小画像」で、JPEG背景や形式変換後の最終ファイルそのものではありません。出力の変換内容とチェックサムはレポート／manifestに記録します。

## 出力と対応範囲

| 形式 | 現在の出力 |
| --- | --- |
| PNG | sRGB、透明度を保持した共通描画結果 |
| JPEG | 品質と不透明背景を明示して出力 |
| WebP | ロスレスのみ。sRGBと透明度を保持 |
| SVG | **アートボード全体をラスタ化**して画像を埋め込む互換SVG。明示承認が必要 |
| PDF | **アートボード全体をラスタ化**するRGB校正用PDF。解像度とラスタ化の明示承認が必要 |

SVG／PDFの文字・図形は編集可能なベクターとして出力しません。フォント埋め込み、CMYK、PDF/X、印刷入稿保証、複数アートボードのPDF統合は未対応です。日本語・英語の横組みを対象とし、縦組み・ルビは未対応です。グループ化は同じ親の隣接レイヤーに限定し、変形・効果を持つグループの解除は描画が変わるため拒否します。詳細は[エンジンの検証記録](reports/editor-engine.md)を参照してください。

## 起動・ビルド

対象はmacOS 13以降のApple Silicon Macです。Node.js 22.12以降、npm、Rust 1.97.0（`rust-toolchain.toml`）、Xcode Command Line Toolsが必要です。

```sh
npm ci
npm run desktop
```

`npm run desktop` はネイティブMacアプリをdebugビルドして開きます。テンプレートと素材を同梱し、製品画面に開発サーバーは使いません。

```sh
npm run desktop:build
open "target/release/bundle/macos/SILIQ Creative Generator.app"
```

上記のデスクトップ操作とRust操作は、Skiaの配布バイナリーを `third_party/skia-binaries.lock` のSHA-256と照合するラッパーを経由します。初回は依存とSkiaアーカイブを取得するため通信します。キャッシュ準備後のRust検査には `--offline --locked` を付けられます。ビルドでは参照用React画面の資産も生成しますが、Macの製品画面はSwiftUIです。ローカルビルドは既定でad-hoc署名を行い、App Sandboxと利用者が選んだファイルへの権限だけを付けます。

```sh
npm run typecheck
npm run test:ui
npm run build
npm run macos:typecheck
scripts/cargo-verified.sh test --workspace --locked
scripts/cargo-verified.sh clippy --workspace --all-targets --locked -- -D warnings
scripts/offline-desktop-audit.sh
# Macの署名済みリリースを作成した後
scripts/test-macos-sandbox.sh
```

静的監査は実行時の依存・機能、制限されたIPC、CSP、通信／AI関連の実装を検査します。署名済み `.app` を引数に渡すと実際の署名・entitlementsも検査します。静的監査だけで実測通信ゼロや全リリース試験の合格を宣言するものではありません。

サンドボックス検査は専用の署名済みアプリで行い、専用コンテナ内の保存・再読込・PNG/PDF書き出し・JSONからの5形式量産を実行します。製品データには触れません。結果と署名の記録は `out/sandbox-probe` に残ります。

CLIの単体PNG生成とCSV量産も残しています。

```sh
scripts/cargo-verified.sh build --release -p render-cli --locked
mkdir -p out
./target/release/siliq-render render \
  --doc fixtures/public-safe/documents/basic.json --variant square \
  --resources fixtures/public-safe/resources \
  --out-dir out --name basic-square.png --report basic-square.report.json
```

## 実装・検証・配布

文書の現行契約は **`0.4.0-core`** で、旧 `0.2.0-core`／`0.3.0-core` も読み込みます。未知の項目や不整合を黙って捨てずに検証します。アプリの版番号と文書スキーマの版番号は別です。

- [Macの製品画面](apps/macos/Sources/)・[C ABI接続口](crates/native-bridge/)・[共通描画エンジン](crates/design-render/)
- [ネイティブ接続の検証](reports/native-macos-bridge.md)・[画面移植の判断](docs/adr/0010-native-macos-interface.md)
- [エンジンのテスト・制約](reports/editor-engine.md)・[保存、IPC、権限、量産の検証](reports/desktop-host.md)
- [作品パッケージと復旧](docs/adr/0007-project-packages-and-recovery.md)・[量産と再開](docs/adr/0008-controlled-batch-jobs.md)・[編集と派生出力](docs/adr/0009-editor-core-and-derivative-exports.md)
- [要件](docs/01_REQUIREMENTS.md)・[アーキテクチャ](docs/02_ARCHITECTURE.md)・[オフライン／安全性](docs/03_SECURITY_AND_OFFLINE.md)・[受入ゲート](docs/04_ACCEPTANCE_AND_PHASES.md)
- [第三者ライセンス](third_party/NOTICE)・[同梱ライセンス本文](third_party/licenses/)・[対象依存SBOM](third_party/sbom-aarch64-apple-darwin.json)

React／Tauriの旧画面は編集契約の参照実装として保持し、`npm run desktop:tauri` で開発起動できます。ネットワーク権限なしのMac SandboxではWebKitの初期化が拒否されるため、製品版の起動対象にはしません。

現在はローカルMac版を確認する段階です。**Developer ID Application証明書がないため、公証済みの一般公開配布は別工程です。** Windows／Linuxのデスクトップ実機動作は未検証で、特にファイル権限処理はPOSIXのディレクトリハンドルを前提としています。初期の設計文書には将来要件と旧段階の記述も含まれるため、実装済みの範囲は上記ADRと検証記録を優先してください。規定環境での速度・メモリー・長時間運用の全測定は完了しておらず、v1全要件やG4全ゲートの完全合格は宣言していません。
