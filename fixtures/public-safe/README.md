# public-safe fixtures

顧客素材・実案件データは含まない。全ファイルは公開・再配布して問題のないもの、またはこのリポジトリー内のscriptで生成した合成物だ。

```
resources/assets/<sha256>.<ext>   # 内容ハッシュで命名（.siliqcg packageと同じ配置）
resources/fonts/<sha256>.<ext>
documents/basic.json              # 機能検査用fixture文書（square / portrait は承認済み、wide-draft は未承認）
documents/showcase.json           # 見本デザイン（架空ブランド。承認状態はfixture用の設定）
licenses/                         # 同梱フォントのライセンス
```

製品仕様のsample（`examples/design.core.sample.json`）とは別物だ。sampleは素材未同梱・variant未承認のまま残し、描画は停止するのが正しい（テストで確認している）。

## ファイル一覧

| ファイル | SHA-256 | 出所 |
|---|---|---|
| `resources/fonts/dff723ba….otf` | `dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073` | Noto Sans JP Regular（SubsetOTF）。`https://github.com/notofonts/noto-cjk` のtag `Sans2.004`（commit `523d033d6cb47f4a80c58a35753646f5c3608a78`）の `Sans/SubsetOTF/JP/NotoSansJP-Regular.otf`。4,533,028 bytes |
| `licenses/NotoSansJP-OFL-1.1.txt` | `6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2` | 同commitの `LICENSE`（SIL Open Font License 1.1） |
| `resources/assets/f2f5342a….png` | `f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3d9a1995bde5cda0` | `scripts/gen_fixture_photo.py` で生成した1600×1000 RGBA（グラデーション、格子、焦点用の円、24pxの透明な縁）。Python 3.13／zlib 1.3で生成。zlibの版が違うと同じscriptでもbytesが変わりうる |
| `resources/fonts/1b0edfb5….otf` | `1b0edfb500b73a4fa8a4fcaae1bbbd403994e08e73e3e0da37e70d3853f42c5f` | Noto Sans JP Bold（SubsetOTF）。Regularと同じtag・commitの `Sans/SubsetOTF/JP/NotoSansJP-Bold.otf`。4,656,448 bytes。同じOFL 1.1 |
| `resources/assets/d8ffb441….png` | `d8ffb441e45dfca1da06544988dcd71d4886242df50798fbbc45784ecc2bc432` | `scripts/gen_showcase_backdrop.py` で生成した1600×1400 RGBの抽象画像（暖色の色面、周辺減光、固定seedの粒子） |
| `resources/assets/a1c4ee01….svg` | `a1c4ee0188b416e60f10fdd9a47d92fb194209448592949344fd3595232ab316` | 架空ブランド「KOHAKU COFFEE ROASTERS」の豆シンボル（手書きSVG） |
| `resources/assets/08317af1….svg` | `08317af1678409f3a1dcf29b651f12b439d311bcddefdffd3580cd4b0067c96c` | 手書きの合成ロゴ（rounded square＋even-odd穴＋円、バー形状。文字なし） |

## フォントのライセンス

Noto Sans JPはSIL Open Font License 1.1で、ソフトウェアへの同梱・再配布が許可されている（単体販売は不可）。ライセンス文は `licenses/` に置いている。document内のフォント宣言は `packagePolicy: "embed-confirmed"`。テスト用fixtureとしての同梱であり、製品へのフォント同梱方針（FNT-002）とは別に扱う。

## showcase.json について

見た目の水準を確かめるための見本。「KOHAKU COFFEE ROASTERS」は架空のブランドで、価格・日付・店舗も架空。Phase 0の機能（矩形、線形グラデーション、画像cover＋clip、ノード不透明度、SVGロゴ、Regular／Boldの2書体、文字の固定サイズ）だけで組んでいる。variantの `approved` はfixtureとして描画するための設定で、作者による承認ではない。
