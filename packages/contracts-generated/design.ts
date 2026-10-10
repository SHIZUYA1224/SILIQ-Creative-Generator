/* Generated from schemas/design.core.schema.json. Run npm run contracts. */

export type DesignDocument = {
  format: "siliq-creative-generator-core";
  schemaVersion: "0.2.0-core" | "0.3.0-core" | "0.4.0-core";
  documentId: string;
  revision: number;
  title: string;
  colorSpace: "sRGB";
  baseArtboard: {
    width: number;
    height: number;
    rootNodeId: string;
  };
  /**
   * @minItems 1
   * @maxItems 2000
   */
  nodes: Node[];
  /**
   * @minItems 0
   * @maxItems 100
   */
  fonts: Font[];
  /**
   * @minItems 0
   * @maxItems 2000
   */
  assets: Asset[];
  /**
   * @minItems 0
   * @maxItems 500
   */
  slots: Slot[];
  /**
   * @minItems 1
   * @maxItems 64
   */
  variants: Variant[];
  /**
   * @maxItems 2000
   */
  nodeStyles?: NodeStyle[];
  brandSnapshot?: BrandSnapshot;
};
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "node".
 */
export type Node = GroupNode | RectNode | TextNode | ImageNode | LogoNode | PathNode;
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "paint".
 */
export type Paint = Solid | LinearGradient | RadialGradient;
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "font".
 */
export type Font = {
  id: string;
  nameHint: string;
  state: "unresolved" | "resolved";
  sha256: string | null;
  faceIndex: number;
  packagePolicy: "reference-only" | "embed-confirmed";
};
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "asset".
 */
export type Asset = {
  id: string;
  kind: "raster" | "vector";
  state: "unresolved" | "resolved";
  sha256: string | null;
  mimeHint: "image/png" | "image/jpeg" | "image/webp" | "image/svg+xml";
};
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "nodeEffect".
 */
export type NodeEffect =
  | {
      kind: "blur";
      radius: number;
    }
  | {
      kind: "shadow";
      color: string;
      opacity: number;
      offsetX: number;
      offsetY: number;
      blur: number;
    };

/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "groupNode".
 */
export interface GroupNode {
  id: string;
  name: string;
  type: "group";
  bounds: Bounds;
  rotationDeg: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  /**
   * @minItems 0
   * @maxItems 2000
   */
  childIds: string[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "bounds".
 */
export interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "rectNode".
 */
export interface RectNode {
  id: string;
  name: string;
  type: "rect";
  bounds: Bounds;
  rotationDeg: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  fill: Paint;
  cornerRadius: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "solid".
 */
export interface Solid {
  kind: "solid";
  color: string;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "linearGradient".
 */
export interface LinearGradient {
  kind: "linear-gradient";
  angleDeg: number;
  /**
   * @minItems 2
   * @maxItems 16
   */
  stops: {
    offset: number;
    color: string;
    opacity: number;
  }[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "radialGradient".
 */
export interface RadialGradient {
  kind: "radial-gradient";
  center: {
    x: number;
    y: number;
  };
  radius: number;
  /**
   * @minItems 2
   * @maxItems 16
   */
  stops: {
    offset: number;
    color: string;
    opacity: number;
  }[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "textNode".
 */
export interface TextNode {
  id: string;
  name: string;
  type: "text";
  bounds: Bounds;
  rotationDeg: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  text: TextSpec;
  fill: Solid;
  outline?: TextOutline;
  shadow?: TextShadow;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "textSpec".
 */
export interface TextSpec {
  value: string;
  fontId: string;
  fontSize: number;
  lineHeight: number;
  letterSpacing: number;
  maxLines: number;
  overflowPolicy: "error" | "fit-approved-sizes";
  /**
   * @minItems 0
   * @maxItems 200
   */
  allowedFontSizes: number[];
  language: "ja" | "en";
  align: "left" | "center" | "right";
  /**
   * @maxItems 32
   */
  features?: FontFeature[];
  kinsoku?: "unicode" | "strict-japanese";
  /**
   * @maxItems 100
   */
  runs?: TextRun[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "fontFeature".
 */
export interface FontFeature {
  tag: string;
  value: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "textRun".
 */
export interface TextRun {
  start: number;
  end: number;
  fontId: string;
  color?: string;
}
/**
 * 0.3.0-core. Visible outline thickness outside the glyph edge, in du.
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "textOutline".
 */
export interface TextOutline {
  color: string;
  width: number;
}
/**
 * 0.3.0-core. Shadow of the glyphs including the outline; blur is a radius in du.
 *
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "textShadow".
 */
export interface TextShadow {
  color: string;
  opacity: number;
  offsetX: number;
  offsetY: number;
  blur: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "imageNode".
 */
export interface ImageNode {
  id: string;
  name: string;
  type: "image";
  bounds: Bounds;
  rotationDeg: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  assetId: string;
  fit: "cover" | "contain" | "stretch";
  focalPoint: {
    x: number;
    y: number;
  };
  clipToBounds: boolean;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "logoNode".
 */
export interface LogoNode {
  id: string;
  name: string;
  type: "logo";
  bounds: Bounds;
  rotationDeg: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  assetId: string;
  fit: "contain";
  preserveAspectRatio: true;
  minWidth: number;
  clearSpace: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "pathNode".
 */
export interface PathNode {
  id: string;
  name: string;
  type: "path";
  bounds: Bounds;
  rotationDeg: number;
  opacity: number;
  visible: boolean;
  locked: boolean;
  fill: Paint;
  data: string;
  viewBox: Bounds;
  fillRule: "non-zero" | "even-odd";
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "slot".
 */
export interface Slot {
  id: string;
  type: "text" | "image";
  required: boolean;
  nodeId: string;
  target: "text.value" | "assetId";
  maxLength?: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "variant".
 */
export interface Variant {
  id: string;
  label: string;
  width: number;
  height: number;
  approvalStatus: "draft" | "approved";
  /**
   * @minItems 0
   * @maxItems 2000
   */
  nodePatches: NodePatch[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "nodePatch".
 */
export interface NodePatch {
  nodeId: string;
  bounds: Bounds;
  fontSize?: number;
  /**
   * 0.3.0-core, text nodes only.
   */
  lineHeight?: number;
  /**
   * @minItems 6
   * @maxItems 6
   */
  affine?: number[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "nodeStyle".
 */
export interface NodeStyle {
  nodeId: string;
  blendMode?: "normal" | "multiply" | "screen";
  stroke?: Stroke;
  /**
   * @maxItems 8
   */
  effects?: NodeEffect[];
  clip?: PathClip;
  mask?: PathMask;
  imageAdjustments?: ImageAdjustments;
  protectedRegion?: NormalizedRect;
  /**
   * @maxItems 100
   */
  protectedFrom?: string[];
  /**
   * @minItems 6
   * @maxItems 6
   */
  affine?: number[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "stroke".
 */
export interface Stroke {
  color: string;
  width: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "pathClip".
 */
export interface PathClip {
  data: string;
  fillRule: "non-zero" | "even-odd";
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "pathMask".
 */
export interface PathMask {
  data: string;
  fillRule: "non-zero" | "even-odd";
  opacity: number;
  invert: boolean;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "imageAdjustments".
 */
export interface ImageAdjustments {
  brightness: number;
  contrast: number;
  saturation: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "normalizedRect".
 */
export interface NormalizedRect {
  x: number;
  y: number;
  width: number;
  height: number;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "brandSnapshot".
 */
export interface BrandSnapshot {
  id: string;
  revision: number;
  /**
   * @maxItems 100
   */
  colors: NamedColor[];
  /**
   * @maxItems 100
   */
  textStyles: NamedTextStyle[];
  /**
   * @maxItems 100
   */
  logoAssetIds: string[];
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "namedColor".
 */
export interface NamedColor {
  id: string;
  name: string;
  color: string;
}
/**
 * This interface was referenced by `undefined`'s JSON-Schema
 * via the `definition` "namedTextStyle".
 */
export interface NamedTextStyle {
  id: string;
  name: string;
  style: TextSpec;
}
