# 03 — 完全オフラインとセキュリティ

## 1. 守る対象と境界

守る対象は、元画像、ロゴ、フォント、顧客名等の文字データ、制作文書、書き出し物、ユーザーの他のローカルファイルだ。

未信頼入力は画像、SVG、フォント、CSV、JSON、独自package、ファイル名、埋め込みmetadata。オフラインであっても入力ファイルの解釈とファイルアクセスには検証が必要になる。

ユーザーOS全体が侵害された場合、管理者がアプリを改造した場合、他のアプリが同じ保存フォルダーを読む場合まで秘密を保証する製品ではない。アプリ自身の通信とアクセスを制限する。ネット断だけであらゆる攻撃を防ぐとは主張しない。

## 2. 通信禁止の実装要件

| 層 | 必須対策 |
|---|---|
| 製品機能 | ログイン、オンライン認証、解析、広告、update check、クラウド同期、外部フォント、URL素材取得を作らない |
| 配布物 | UI・アイコン・辞書・Unicode資源・必要なnative libraryを同梱。初回ダウンロードを発生させない |
| JavaScript | 外向きfetch／XHR／WebSocket／EventSource、外部navigation、フォーム送信、外部scriptを禁止 |
| Tauri | HTTP・WebSocket・localhost・updater等の不要pluginを採用しない。必要capabilityだけ明示 |
| Rust／worker | 外部HTTP clientやAI SDKを導入しない。必須でないnetwork featureを有効にしない |
| OS | macOS releaseはApp Sandboxとnetwork entitlement不付与を検証。workerも対象に含める |
| 検証 | 配布物監査＋通信観測＋ネット遮断実機試験＋不正入力試験 |

CSPはWebViewの制御であって、Rustやnative libraryからの通信を禁止するOS firewallではない。Tauri capabilityもRustの不適切な実装を自動的に安全にしない。両者に加えてnative側の設計とOS側の試験を行う。[S01]

CSPの設定方針はdefault deny、外部frame/object禁止、外部navigation禁止、ローカルの同梱scriptだけ許可。Tauri IPCに必要な内部scheme／originは公式設定と実機で確認して最小許可する。`connect-src 'none'`を盲目的に設定してIPCを壊したり、逆に`*`で解決したりしない。

カスタムschemeがURLの形を持つことと、TCP接続が発生することは区別する。製品にローカルHTTP serverは不要。worker間はpipe等で完結させる。

macOSのOS自身による署名・信頼確認等は、アプリによる通信とは区別して記録する。ただしオフライン初回起動の受け入れを省略する理由にはしない。署名・notarization・stapling等の配布手順を検証し、GatekeeperやOS保護の全体無効化を利用手順にしない。

## 3. ファイル権限

入力ファイルはユーザーの選択で許可を得る。フォルダーの一括取り込みは選択したフォルダーの読取だけ。出力は選択した出力ディレクトリーの書込だけ。アプリ専用workspace・cache・recoveryはアプリが管理する。

Hostはファイル選択結果を不透明なgrant ID／asset IDへ変換する。フロントエンドへ`read_any_path`や`write_any_path`のような汎用APIを公開しない。

Tauriのカスタムcommandは、登録しただけで自動的に用途別に制限されると考えない。command manifest／capabilityの露出と、各command内部のgrantチェックの双方を検証する。[S01]

絶対パス、`..`、NUL、symlink、hardlink、同名競合、予約ファイル名、大小文字やUnicode正規化による衝突を検査する。文字列prefix比較だけでディレクトリー内と判定しない。許可したdirectory handleからの相対操作等を用い、確認後に差し替わる競合にも備える。

再起動後のフォルダー利用権限の保持は、OSが提供する安全な仕組みとユーザーの明示設定に限定する。プロジェクトを閉じる操作と、永続許可を取り消す操作の違いをUIに示す。

## 3.1 保存先・入力元のネットワーク境界

v1はローカル実体のある通常ファイルだけを取り込む。SMB、UNC、WebDAV等のネットワーク共有やリモートmountを入力・出力対象にしない。クラウドにしか実体のないplaceholderの取得をアプリから要求しない。パス検査のために不用意にリモート接続を発生させないことも試験する。

既定workspaceはアプリ専用のローカル領域に置く。iCloud等の既知の同期領域をユーザーが指定した場合は、アプリ自体が通信しなくても他のプロセスがファイルを送信しうることを警告する。すべての独立した同期ソフトを検出・停止することは保証しない。アプリによる通信禁止と、OS全体を含めたデータ持ち出し禁止の境界を混同しない。

## 4. SVGの安全な取り込み

SVGをinnerHTML等でWebView DOMへ挿入しない。XML parserで読み、許可された要素・属性だけを独自scene nodeへ変換する。

基本の許可対象はpath、rect、circle／ellipse、line／polyline／polygon、group、transform、限定したfill／stroke、linearGradient／radialGradient、clipPath等。許可対象でも数、深さ、座標、参照回数に制限を設ける。

script、event handler、foreignObject、外部href／url、外部CSS、DTD／entity解決、animation、ネットワークリソースは拒否する。内部use参照も展開数・循環・深さを検査する。SVGに含まれる文字は、v1で取り込み非対応の場合は明確に拒否し、outline済み素材等をユーザーが用意する。

「危険な要素を削ったので正常」とせず、外観に影響する削除・未対応がある場合はインポート失敗として示す。暗黙の画像化はしない。

## 5. ラスター・フォント・package

ラスターは拡張子だけで判定せず、ヘッダーとdecode結果を確認する。寸法overflow、巨大展開、壊れたICC、無限処理、過剰metadataを防ぐ上限を持つ。フォントもparser入力であり、単なる安全な文字データではない。サイズ・face数・処理時間を制限し、失敗を製品全体のクラッシュにしない。

packageはZIP bomb、Zip Slip、重複entry、symlink entry、巨大JSON、参照循環、checksum不一致を検査する。展開前後のサイズと総byte数を計数する。暗号化されたZIPや不明なentry typeをv1で無理に扱わない。

checksumは破損・取り違えを検出するためのものだ。攻撃者がchecksumも書き換えられるため、署名や信頼できる出所の証明ではない。検証済みchecksumがある入力も未信頼としてparseする。

## 6. 初期リソース制限案

以下は実測で調整する初期ガード値であり、性能保証ではない。値の引き上げは資源見積もりと否定試験を伴う。

| 対象 | 初期上限案 |
|---|---|
| 1枚の取り込み画像 | 圧縮100MB、64MP、各辺16,384px |
| 1枚の出力 | 64MP、各辺16,384px、資源予約に収まること |
| 文書 | 2,000 node、32段のgroup、64 artboard |
| SVG | 入力10MB、展開後node／path commandに個別上限 |
| JSON／CSV | 各20MB、datasetは10,000行までを初期検証対象 |
| package | 展開後合計2GB、10,000 entry。圧縮比も監視 |
| worker | 同時最大2、1jobの推定作業領域1GiB以内を初期基準 |

重い画像効果では同じ出力寸法でも使用資源が増える。表のpixel上限に収まるから必ず描画を許可するわけではない。資源予測が上限を超えた場合は開始前にE_RESOURCE_LIMITを返す。

## 7. データの所在・保持・削除

元素材と文書はユーザーの保存package／ローカルworkspace。cacheとrecoveryはアプリ専用領域。ジョブsnapshotと出力manifestはローカルのjob領域に置く。

診断ログはevent code、revision、node ID、row ID、所要時間等を既定とし、画像本体、本文、顧客名、元ファイルの絶対パスを記録しない。詳細レポートの生成は明示操作とする。外部送信機能は設けない。

設定画面で保存場所・cache量・recovery量・job履歴量を確認でき、種類別に削除できる。作業中文書や未完了jobが参照している素材をGCで消さない。削除は通常のファイル削除であり、SSD上の完全消去を保証すると表示しない。

アプリは暗号化ストレージをv1の必須にしない。ローカル保存は「同じ端末の他ユーザーから常に秘匿される」と同義ではない。OSのアカウント・disk暗号化等の責任範囲を文書化する。

## 8. 開発支援の境界

Claude Codeに与えるのは設計書と、権利・機密性を確認したテストデータ。顧客素材、実案件CSV、実フォントライセンス情報、業務資料は、別途許可なく開発支援へ渡さない。

`CLAUDE.md`は開発者への指示であり、機械的な権限制約の代わりではない。Claude Code公式もこの区別を説明している。実装側では依存検査、権限設定、試験、reviewを必須にする。[S11]

Claude Codeの実行ファイル、認証情報、API key、会話履歴、MCP接続、モデルを製品packageへ同梱しない。アプリ内部にClaude Codeを呼び出すボタンを付けない。

## 9. 更新と依存管理

lockfile、native依存のrevision、checksum、feature一覧、SBOM、ライセンス一覧をreleaseごとに保存する。取得済み依存の検証と、開発用・test用・runtime用の区別を行う。

更新は署名・checksumを確認したローカルinstallerによる手動導入を基本とする。アプリが更新の有無を問い合わせる機能は作らない。脆弱性情報の確認は開発・保守側の工程であり、アプリの通信機能を復活させる理由にしない。

## 10. リリース拒否条件

アプリ由来の外部通信、実行時AI推論、無断ファイルアクセス、未知SVGの黙殺、無許可フォント置換、保存データの無通知破損のいずれかを再現した場合はリリースを止める。READMEの注意書きで必須条件の不合格を覆い隠さない。
