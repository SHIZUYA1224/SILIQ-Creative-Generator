# Native performance measurement — 2026-10-10

**100行×3variant の300 PNGは失敗・欠落なしで検証できたが、226.531秒で180秒目標は未達。** 単品の反復 CLI wall time は p50 435.934ms / p95 657.071ms。これは M1 Max・32GB の測定であり、仕様の M1・16GB 基準機の合格結果として扱わない。

## 入力と実行条件

[固定fixture](../fixtures/performance-safe/documents/standard.json) は 1,080×1,350、120 visible drawable layers とroot group（計121 nodes）、固有合成 PNG 8枚（各1,600×1,000、1.6MP）、日本語＋English、Noto Sans JP Regular / Bold / Black 3face、linear/radial gradient、image clip 8個、native shadow 8個を含む。[100行CSV](../fixtures/performance-safe/data/100-rows.csv) と3つのQA承認済みvariantを固定した。画像は低entropyの合成パターンであり、実写真全般の代表値ではない。顧客素材・実写真は使わない。フォントは既存OFL fixtureの未改変copy。全input/resource hashは [fixture-manifest](../fixtures/performance-safe/fixture-manifest.json) に収録する。

| 条件 | 実測・記録 |
|---|---|
| CPU / RAM | Apple M1 Max、10 logical CPUs、32GiB |
| OS / target | macOS 26.3.1、build 25D2128、arm64 |
| build | Rust 1.97.0 release optimized、checksum-verified Skia 0.153.3 CPU raster |
| font backend | platform-from-data、明示font 3face、fallback無し |
| codec | Skia libpng、all filters、zlib level 6、text chunk無し |
| color / sampling | sRGB、RGBA8888 premultiplied surface、linear filter/mipmaps |
| 電源 | 終了時 Battery Power・84%。開始時の状態と切替時点は未記録（準備時はAC）。電源固定試験ではない |
| 他の負荷 | 測定中はrootのrelease build/clippyを停止。background/desktop appは終了しない。nativeUIは停止。背景CPU負荷を完全には除去していない |
| load average | 開始 5.65 / 9.60 / 7.86、終了 5.55 / 9.54 / 8.41（1/5/15分） |
| storage / screen | ローカルworkspace。SSD性能の独立profileと画面scaleは未取得。headless CLI試験 |

実行ファイル SHA-256 は `ff9955f9025ff39144c7ec94b90b40f9a80786369dc22804a6d851458ea7623e`、native build fingerprint は `39bfbf74644a4983a889c1f564fab8e7a586c6c14b9c7a54e0119bd15ffd6fd7`。これはこの測定に使ったbuildの記録であり、後続のhost/Sandbox/logo修正を含むreleaseの性能を測り直したものではない。

## 数値と検証

`scripts/measure-performance.py` は `/usr/bin/time -l` でCLI processのpeak RSSを取得し、外部monotonic clockで起動から終了までを測る。最初の1回＋反復20回、p50/p95はnearest-rank。各回は新CLI processとResourceStoreであり、素材は毎回native decodeする。OS file cacheは温まり得るが、事前decode済みのnative warm profileではない。RenderReport自体にはstage timing fieldが無く、内部decode/layout/draw/encode時間は分離していない。

| 項目 | 結果 | 仕様の扱い |
|---|---|---|
| 最初のCLI単品 | 500.989ms | filesystem cacheを消していないためtrue coldではない |
| 反復単品 p50 / p95 | 435.934 / 657.071ms、n=20 | この条件で1秒未満。warm native/M1 16GBの合格宣言にはしない |
| 反復単品最大wall | 707.277ms | 起動・decode・描画・PNG・disk publishを含む |
| 単品 peak RSS | 最大196.8MiB | CLI processのみ |
| 100×3 PNG batch wall | **226.531秒**（CLI report 226.50秒） | **180秒目標未達、46.531秒超過** |
| batch peak RSS | 957.4MiB（1,003,880,448 bytes） | Host＋WebView＋worker合計2GiBの測定ではない |
| 出力検証 | 300/300、300固有SHA-256、ERROR/WARNING 0 | 全ファイル名、PNG header、dimension、hash、row/variant/format identity、manifest/input digest照合 |
| 単品再現 | 21/21 同一PNG hash | 同じtarget/build/profileでのbytes再現 |

単品PNG SHA-256：`c676ffc23249ac8359ce176559c1c8eaa05862d9a94f240eed1d815d71d18435`。batch manifest SHA-256：`01a854ebcc96c0422852695658413516b9387d3d8d010725d00b1cb4b84c2eff`。

batch は既定block-allで全300件のnative preflightを済ませ、その後300件を再描画する現実装を測る。durable journal、SHA検証、no-clobber publication、最終manifestまで含む。Desktopでのpicker、plan確認、開始前に別途行うpreflight API、UI表示と応答は含まない。180秒を超えた原因をstage profileで切り分ける作業は未実施。改善時にもblock-allのエラー検査、固定snapshot、整合性検証、原本・既存出力の保護を維持する。

## 未測定の受入条件

仕様の起動p95 5秒、操作反応p95 50ms、native→転送→表示のwarm preview p95 250ms、Host/WebView/worker合算2GiB、UI操作可能なbatch、checkpointの2秒条件、帰属付き通信観測、機内モード初回操作、M1 16GB基準機はこの試験の対象外。CLIが描画できたことをそれらの合格の代わりにしない。

## 再実行と証跡

```sh
python3 scripts/generate-performance-fixture.py
scripts/cargo-verified.sh build --release -p render-cli --locked
# 他のbuild/testを停止してから実行する。新規run directoryを使う。
python3 scripts/measure-performance.py
```

生成scriptの同一入力再実行とnative schema checkは通過した。PNG圧縮bytesはPython/zlib版に依存し得るため、fixture manifestに生成環境（今回zlib 1.2.12）とhashを保存する。計測結果は `out/performance/run-20261010-standard/summary.json`、21件すべてのRenderReportとtime/RSS log、batch stdout/time log、300PNGとmanifestに保存した。`out/` は公開repoへcommitせず、集計と固定fixtureをこのreportから参照する。
