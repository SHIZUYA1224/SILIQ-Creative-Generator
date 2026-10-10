# 報告 — 文字のフチ・影＋CSV一括生成（最小版）

報告日：2026-10-10。実行環境はStep Aと同じLinux x86_64のクラウドコンテナ（Rust 1.97.0、skia-safe 0.153.3、システムFreeType 2.13.2）。**macOSでは未実施。**

## 実装した範囲

| 変更 | 内容 |
|---|---|
| schema 0.3.0-core | `text.outline`、`text.shadow`、`nodePatch.lineHeight`。0.2.0-coreも引き続き読む（ADR 0005） |
| `design-model/src/binding.rs` | CSV行→slotへの差し込み（宣言された属性だけ、値は原文のまま） |
| `design-render` | フチ・影の描画（3回組んで改行一致を照合）、`render_document`、照合済みbytesのsnapshot cache |
| `crates/job-runner` | 厳格なCSV parser、出力名の検査、一括生成（全件検査→一時領域→全件公開、manifest） |
| `siliq-render batch` | CLIサブコマンド |
| fixture | `retail-promo-template.json`（0.3、差し替え欄5つ、縦長＋正方形）、`data/retail-promo-stores.csv`（架空10店舗） |

## 結果（実測）

```
siliq-render batch --doc fixtures/public-safe/documents/retail-promo-template.json \
  --data fixtures/public-safe/data/retail-promo-stores.csv --variants portrait,square \
  --resources fixtures/public-safe/resources --out-dir <dir>
```

- 10行×2版＝PNG 20枚＋manifest。所要時間は1.70秒、1.70秒、1.74秒（release build、逐次、3回）。この環境での参考値で、性能の保証ではない。
- 3回の実行で、manifestと20枚すべてのSHA-256が一致した。
- 価格が6桁の行（10,800／11,800／12,800）は、縦長版で承認済みサイズの250→230へ下がった（manifestの `appliedFontSizes` に記録される）。正方形版は210のまま。警告は0件。
- 一覧：`reports/batch/contact-sheet.png`（一覧画像自体もこのエンジンで組んだ）。個別の例：`reports/batch/store-*.png`。manifest：`reports/batch/manifest.json`。

### 全件停止の実例

`reports/batch/blocked-example.csv` は、正常な10行に不正な3行を加えたもの。実行結果は終了コード2で、**PNGは1枚も作られない**（正常な10行の分も出さない）。

| 行 | 問題 | code |
|---|---|---|
| store-011 | 支店名にタイ文字（フォントに字形なし） | `E_GLYPH_MISSING`（U+0E20, U+0E29, U+0E32） |
| store-012 | 価格「1,234,567」が欄の上限7文字を超える | `E_DATA_INVALID` |
| store-013 | 電話番号が空欄 | `E_DATA_INVALID` |

各issueに行・版・ノードが付く（`reports/batch/blocked-example.issues.json`）。

## テスト

`scripts/cargo-verified.sh test --workspace`：**125件成功、失敗0、ignore 0**。

| テスト | 件数 |
|---|---|
| design-model（schema 0.3、版ゲート、差し込みを含む） | 38 |
| design-svg | 25 |
| design-render（フチ・影の画素、改行一致、ぼかしの再現性を含む） | 36＋1 |
| job-runner csv（引用、改行、BOM、先頭0、数式非評価、非UTF-8拒否、行数上限） | 9 |
| job-runner batch（20枚＋manifest、再現性、1行不正で全停止、未知・欠落列、重複・危険なrowId、上書き禁止、版、名前パターン） | 8 |
| render-cli（batchサブコマンドを含む） | 8 |

clippy（`-D warnings`）合格。既存の0.2文書5件の出力hashは変更前と同一。fixture文書4件と製品sampleは、更新したJSON Schema（Python jsonschema 4.26）でもエラー0件。

途中で失敗したテストが2件あった。どちらもテスト側の前提の誤り（差し込み値がテンプレートと同じだった、「未対応の版」に0.3を使っていた）で、テストを修正した。

## 制限・未実装

- キャンセル・再開・失敗分の再実行、部分出力モード、並列化は未実装（ADR 0006）。
- 要素同士の重なりは検出しない。今回、価格のカンマと注記の重なりを目視で見つけ、テンプレート側で直した。
- 画像slotは、テンプレートに登録済みの素材idを選ぶだけ。店舗ごとの写真を取り込む機能は無い。
- macOS（CoreTextのフォント経路）での描画・速度は未検証。

## 次の小さな作業（候補）

1. 画像slotへの店舗写真の取り込み（フォルダー単位の許可、hash登録）
2. 要素の重なり検査（作者が「重なってはいけない」と指定した要素の組）
3. 中断・再開（manifestの `inputDigest` を使う）
4. Step B（Mac実機）
