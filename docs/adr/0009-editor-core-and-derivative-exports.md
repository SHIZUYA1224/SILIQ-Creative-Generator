# ADR 0009 — 編集可能な core 0.4 と明示的な派生書き出し

状態：採用／2026-10-10

## 決定

正本は Rust の `DesignDocument` とし、schema を `0.4.0-core` に拡張する。`0.2.0-core` と `0.3.0-core` は引き続き読む。追加フィールドは既定値を持ち、旧形式で新しい機能を指定した場合は拒否する。未知のフィールド、未宣言の参照、非有限値、範囲外の値も拒否する。

core 0.4 では編集可能な複合パス、放射グラデーション、線、normal/multiply/screen、形状の影・ぼかし、パスの clip/alpha mask、画像の fit/focal point/明示的色調補正、宣言済みフォントによる文字 run と OpenType feature を保存する。機能は共通の Skia CPU 描画で preview と書き出しに反映する。

パスは有限の M/L/H/V/C/S/Q/T/Z を受け付け、相対座標と smooth control を展開する。座標絶対値は 1,000,000、文字列は 1 MiB、展開 segment 数は 200,000 までとする。arc などの未対応 command は元の文字列を残してエラーを表示する。UI の anchor/control 編集は明示操作の時だけ canonical path を生成する。安全な外部 SVG の読み込み範囲は ADR 0002 のままであり、編集パスの追加を理由に外部 SVG の許可範囲を広げない。

## グループと版

グループの移動・resize は子の文字サイズや座標を再計算せず、保存する affine 行列で実行する。行列 `[a,b,c,d,e,f]` は `x'=a*x+c*y+e, y'=b*x+d*y+f` の順序で、共通回転の後に掛ける描画合成は `R * Affine` とする。グループ自身の bounds は操作後の frame を表すため、その frame の測定では自身の affine をもう一度掛けない。子の描画では祖先の affine を掛ける。

版の `nodePatch.affine` は明示的な group override とし、解決後の `node_styles` に上書きする。旧版の bounds patch から暗黙の resize を導かない。既存グループを変形する編集は locked な子を含む場合に拒否する。group 作成は同じ親の隣接 layer に限り、paint order と ID を保持する。ungroup は描画を変えない neutral group に限り、回転・affine・opacity・効果のある group を勝手に flatten しない。

## 文字とブランド

文字の既定改行は旧形式の Unicode のままとする。作者が `strict-japanese` を指定した日本語テキストには、明示した禁則文字群を使った改行制約を適用し、元の文字列と UTF-8 byte offset を維持する。文字 run は UTF-8 境界上の非重複範囲と宣言済み font ID、任意色を保存し、サイズ・行送りは枠全体から継承する。feature は 4 桁の ASCII tag と整数値を有限個渡す。縦組み・ルビ・任意 script 実行は導入しない。

BrandSnapshot は名前付き色、TextSpec、許可 logo asset ID と revision を正本に保存する。font/asset は文書の解決済み SHA-256 参照に固定する。logo は contain と縦横比を維持し、祖先の非一様 scale/shear も拒否する。最小幅は変形後の実寸で判定する。clear space はアートボード端と作者が `protectedFrom` で指定した対象の frame に対して確認する。対象の推測や画像内の物体認識は行わず、回転した形の比較は保守的な軸平行外接矩形を用いる。

## 派生書き出し

`design-export` は正規の描画結果だけを受け、パスにアクセスしない。PNG は共通描画の PNG をそのまま返す。JPEG は作者が指定した背景に透明部分を合成し、品質 1..100 を使う。WebP は純 Rust codec の lossless VP8L を使用し、sRGB ICC と straight alpha を保持する。lossy WebP は未対応として拒否する。

SVG/PDF は作者の `allowRasterization=true` を必須とする。現在の互換 SVG と RGB 校正 PDF はアートボード全体の同一 PNG を埋め込む方式であり、report に変換、解像度、文字編集不可、フォント埋め込みなしを記録する。編集可能なベクター SVG、CMYK、PDF/X、印刷入稿保証は提供しない。使わない quality/background 設定を黙って無視しない。

## 資源と再現性

個別画像の上限に加え、font load と pixel decode の前に文書全体の working set を予測する。output/native/RGBA/encoder、最大同時 offscreen layer、順序付き効果、文字影、全参照画像の header pixel size、最大 decode scratch、font bytes、検証済み資源 cache を合算し、1 GiB を超える場合は `E_RESOURCE_LIMIT` で停止する。decode 後も実画像量を合算する。ResourceStore の検証済み byte cache は 512 MiB までとし、派生 codec にも 1 GiB の予測上限を設ける。予測は保守的な拒否に用い、OS 全体の peak RSS を保証する計測値として扱わない。

描画 report の build fingerprint は pinned toolchain/依存 lock、target/profile、model、SVG、renderer、ICC、派生 export と job runner の出力・再開に影響するソースを hash する。旧実行の途中から新しいアルゴリズムで不足分を作ることを防ぐため、batch input digest に同じ immutable identity を含める。

## 検証と境界

Mac の CPU backend で engine 所有 crate の 136 tests が通過した。複合曲線の穴・stroke、radial、blend、clip/mask の背景保持、実際の影・ぼかし画素、文字 run、禁則 offset、base/variant group affine の画素、logo 制約、WebP orientation/ICC、lossless alpha round trip、累積資源予測、旧文書、編集の原子性と Undo を含む。

この ADR は editor engine と出力形式の決定であり、G4 の配布・署名・OS sandbox・全 release gate の合格宣言ではない。共通 renderer の platform 別再現性、worker の強制メモリ制限、長時間 batch の計測、広範な fuzz corpus は release 検証で別途記録する。
