# ADR 0001 — Phase 0 の描画規約とRenderProfile

状態：Phase 0で採用（作者の確認待ち）／2026-10-10

## 背景

中核schema（`schemas/design.core.schema.json`）は、グラデーション角度の向きや、文字の縦位置、ロゴの配置基準などを定めていない。エンジンが暗黙に決めると、作者の意図と異なる見た目が「仕様」として固定されてしまう。そこでPhase 0で採用した規約をここに明記する。変更する場合はschema版・engine版のどちらかを上げる。

## 決定

### 座標
- `bounds` はアートボード左上原点の絶対座標（du）。1du＝1px（scale 1）。親groupの `bounds` は子の位置をずらさない。
- `rotationDeg` は `bounds` 中心まわりの時計回り回転（y軸下向き）。groupの回転は子孫全体に掛かる。
- `opacity < 1` のノードは独立したレイヤーへ描いてから合成する（group opacity）。

### 塗り
- 線形グラデーション：0°＝左→右、90°＝上→下（時計回り）。グラデーション線は `bounds` 中心を通る。0と1のstopが箱の角に接する長さ（CSSと同じ長さの決め方。向きの基準はCSSと異なる）にしている。
- 補間：非premultiplied、出力色空間（sRGB）で補間、端はclamp。
- stopの `opacity` はその色のalpha。

### 画像（`image` ノード）
- `cover`：倍率＝max(枠幅/画像幅, 枠高/画像高)。`contain`：倍率＝min(…)。
- 位置：focal pointを枠の中心へ寄せる。ただし画像が枠を覆う範囲（containなら枠内）にclampする。
- `clipToBounds` はschema上常にtrue。枠でanti-aliasedのクリップを掛ける。
- sampling：線形補間＋線形mipmap（固定）。シャープ化はしない。
- 画像が拡大表示される場合は `W_IMAGE_UPSCALED` を警告としてreportに残す。出力は止めない。

### 文字（`text` ノード）
- 文字列は枠の **上端** から組む（縦方向の中央揃え等はPhase 0では無い）。
- `lineHeight` は絶対値（du）。fitで小さいサイズを適用しても `lineHeight` は作者の値のまま変えない。
- 強制strutで全行の高さを `lineHeight` に揃え、余白は上下に半分ずつ配分する（half leading）。
- hinting off、SkParagraphのrounding hack off。
- `letterSpacing` はdu単位でそのまま適用する。fit時にも変えない。
- 収まりの判定：行数 ≤ `maxLines`、組んだ高さ ≤ 枠高＋0.01、最長行 ≤ 枠幅＋0.01。
- fit：`fit-approved-sizes` の場合、`allowedFontSizes`（降順）のうち `fontSize` 以下を順に試す。最初に収まったサイズを採用し、試した列と採用値をreportへ記録する。どれでも収まらなければ `E_TEXT_OVERFLOW` で停止する。
- 改行は `\n`（U+000A）だけを手動改行として扱う。それ以外の制御文字は `E_GLYPH_MISSING` で拒否する。

### ロゴ（`logo` ノード）
- contain、縦横比固定、枠内で **中央配置**（SVGの `xMidYMid meet` と同じ）。
- 描画幅が `minWidth` 未満なら `E_LOGO_MIN_WIDTH` で停止する。
- `clearSpace` の検査は **未実装**（Phase 1）。何を保護対象とするかの定義が必要なため。

### 出力
- surface：RGBA8888、premultiplied、sRGB、初期値は透明。
- PNG：Skiaのlibpng encoder、全filter、zlib level 6、textチャンク無し。

## 代替案と保留

- 文字の縦中央揃え、ロゴの左揃えなどは、schemaにフィールドを追加して作者が選べるようにするのが正しい。Phase 1でschemaを拡張するまで、暗黙の切替は入れない。
