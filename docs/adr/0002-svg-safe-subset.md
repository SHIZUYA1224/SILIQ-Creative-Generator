# ADR 0002 — SVGロゴの安全なサブセット

状態：Phase 0で採用／2026-10-10

## 決定

SVGはroxmltree（DTD禁止、node数上限）で読み、`design-svg` crateがallowlistの範囲だけを独自のpath表現へ変換する。許可外の要素・属性・値はすべてerrorにする。何かを削って続行することはない。

### 許可
- 要素：`svg`、`g`、`defs`（中は `linearGradient` だけ）、`title`、`desc`、`path`、`rect`（`rx`/`ry` 可）、`circle`、`ellipse`、`polygon`、`polyline`、`linearGradient`、`stop`
- path命令：`M L H V C S Q T Z`（相対・絶対）
- 属性：`fill`（`#rgb`、`#rrggbb`、`black`、`white`、`none`、`url(#id)`）、`fill-opacity`、`fill-rule`、図形の `opacity`、`transform`（matrix／translate／scale／rotate／skewX／skewY）、`stroke="none"`
- `linearGradient`：`x1 y1 x2 y2`（数値、または objectBoundingBox 時の%）、`gradientUnits`、`gradientTransform`、`spreadMethod="pad"`
- ルート：`viewBox`、または単位なし／`px` の `width`・`height`。`preserveAspectRatio` は `xMidYMid meet` だけ

### 拒否（エラーの種類）
- `E_EXTERNAL_REFERENCE`：`#` 以外を指す `href`（名前空間を問わない）、`url(#…)` 以外の `url()`。**他の検査より先に** 文書全体を走査して検出する。
- `E_SVG_UNSUPPORTED`：`script`、`foreignObject`、`use`、`image`、`a`、`text`／`tspan`、`style` 要素と `style`／`class` 属性、`on*` イベント属性、`filter`、`mask`、`clipPath`、`radialGradient`、`animate` 等、stroke、arc命令（`A`）、group opacity、SVG以外の名前空間、DTD／entity、処理命令（`xml-stylesheet` 等）、`mm` 等の単位、`currentColor`／`rgb()` 等の色指定、`line`（線は塗れないため）
- `E_RESOURCE_LIMIT`：入力10MB超、要素10,000超、深さ32超、path segment 200,000超、絶対値1e7超の数値

### 意味の扱い
- objectBoundingBoxのグラデーションは、曲線の極値まで含めた正確なbboxで解決する。
- 変形は頂点と制御点へ直接適用する（アフィン変換に対して正確）。グラデーションは行列として保持し、非一様な変形でも正しく扱う。
- SVGのviewportに合わせ、描画は `viewBox` でクリップする。
- 塗りのある `polyline` はSVGの規定どおり閉じた形として塗る。

## 理由

「危険な要素を削ったので正常」とせず、外観に影響する未対応は取り込み失敗として示す（docs/03 §4）。汎用SVGレンダラー（usvg、SkiaのSVGモジュール）は多くの機能を解決・変換してしまうため、Phase 0では採用しない。

## 今後

stroke（outline化して取り込むか、線描画を正式に対応するか）、arc、clipPathはPhase 1以降で、作者の承認する変換経路として検討する。
