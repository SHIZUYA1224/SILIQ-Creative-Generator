# public-safe fixtures

顧客素材・実案件データは含まない。全ファイルは公開・再配布して問題のないもの、またはこのリポジトリー内のscriptで生成した合成物だ。

```
resources/assets/<sha256>.<ext>   # 内容ハッシュで命名（.siliqcg packageと同じ配置）
resources/fonts/<sha256>.<ext>
documents/basic.json              # 描画用fixture文書（square / portrait は承認済み、wide-draft は未承認）
licenses/                         # 同梱フォントのライセンス
```

製品仕様のsample（`examples/design.core.sample.json`）とは別物だ。sampleは素材未同梱・variant未承認のまま残し、描画は停止するのが正しい（テストで確認している）。

## ファイル一覧

| ファイル | SHA-256 | 出所 |
|---|---|---|
| `resources/fonts/dff723ba….otf` | `dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073` | Noto Sans JP Regular（SubsetOTF）。`https://github.com/notofonts/noto-cjk` のtag `Sans2.004`（commit `523d033d6cb47f4a80c58a35753646f5c3608a78`）の `Sans/SubsetOTF/JP/NotoSansJP-Regular.otf`。4,533,028 bytes |
| `licenses/NotoSansJP-OFL-1.1.txt` | `6a73f9541c2de74158c0e7cf6b0a58ef774f5a780bf191f2d7ec9cc53efe2bf2` | 同commitの `LICENSE`（SIL Open Font License 1.1） |
| `resources/assets/f2f5342a….png` | `f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3d9a1995bde5cda0` | `scripts/gen_fixture_photo.py` で生成した1600×1000 RGBA（グラデーション、格子、焦点用の円、24pxの透明な縁）。Python 3.13／zlib 1.3で生成。zlibの版が違うと同じscriptでもbytesが変わりうる |
| `resources/assets/08317af1….svg` | `08317af1678409f3a1dcf29b651f12b439d311bcddefdffd3580cd4b0067c96c` | 手書きの合成ロゴ（rounded square＋even-odd穴＋円、バー形状。文字なし） |

## フォントのライセンス

Noto Sans JPはSIL Open Font License 1.1で、ソフトウェアへの同梱・再配布が許可されている（単体販売は不可）。ライセンス文は `licenses/` に置いている。document内のフォント宣言は `packagePolicy: "embed-confirmed"`。テスト用fixtureとしての同梱であり、製品へのフォント同梱方針（FNT-002）とは別に扱う。
