//! Rust types mirroring `schemas/design.core.schema.json` (0.2.0-core).
//!
//! Every struct rejects unknown fields. Range and pattern rules from the
//! schema that serde cannot express are enforced in `validate.rs`.

use serde::{Deserialize, Deserializer, Serialize};

pub const FORMAT: &str = "siliq-creative-generator-core";
/// Newest schema version this engine writes and reads.
pub const SCHEMA_VERSION: &str = "0.4.0-core";
/// Every version this engine reads. 0.3.0-core adds text `outline` / `shadow`.
pub const SUPPORTED_SCHEMA_VERSIONS: &[&str] = &["0.2.0-core", "0.3.0-core", "0.4.0-core"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesignDocument {
    pub format: String,
    pub schema_version: String,
    pub document_id: String,
    pub revision: u64,
    pub title: String,
    pub color_space: String,
    pub base_artboard: BaseArtboard,
    pub nodes: Vec<Node>,
    pub fonts: Vec<Font>,
    pub assets: Vec<Asset>,
    pub slots: Vec<Slot>,
    pub variants: Vec<Variant>,
    /// Optional styles keyed by stable node ID, introduced in 0.4.0-core.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub node_styles: Vec<NodeStyle>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brand_snapshot: Option<BrandSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedColor {
    pub id: String,
    pub name: String,
    pub color: String,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NamedTextStyle {
    pub id: String,
    pub name: String,
    pub style: TextSpec,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrandSnapshot {
    pub id: String,
    pub revision: u64,
    pub colors: Vec<NamedColor>,
    pub text_styles: Vec<NamedTextStyle>,
    pub logo_asset_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaseArtboard {
    pub width: f64,
    pub height: f64,
    pub root_node_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum Paint {
    #[serde(rename = "solid")]
    Solid { color: String },
    #[serde(rename = "linear-gradient", rename_all = "camelCase")]
    LinearGradient { angle_deg: f64, stops: Vec<GradientStop> },
    #[serde(rename = "radial-gradient", rename_all = "camelCase")]
    RadialGradient {
        center: FocalPoint,
        radius: f64,
        stops: Vec<GradientStop>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GradientStop {
    pub offset: f64,
    pub color: String,
    pub opacity: f64,
}

/// Text fill: the schema allows only a solid paint here.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolidFill {
    pub kind: SolidKind,
    pub color: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SolidKind {
    #[serde(rename = "solid")]
    Solid,
}

/// Text outline. `width` is the visible thickness outside the glyph edge in du.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextOutline {
    pub color: String,
    pub width: f64,
}

/// Text shadow. `blur` is a radius in du (0 = hard shadow).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextShadow {
    pub color: String,
    pub opacity: f64,
    pub offset_x: f64,
    pub offset_y: f64,
    pub blur: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Font {
    pub id: String,
    pub name_hint: String,
    pub state: ResolveState,
    /// Required key whose value may be `null` (schema: required, nullable).
    #[serde(deserialize_with = "nullable")]
    pub sha256: Option<String>,
    pub face_index: u32,
    pub package_policy: PackagePolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ResolveState {
    Unresolved,
    Resolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PackagePolicy {
    ReferenceOnly,
    EmbedConfirmed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Asset {
    pub id: String,
    pub kind: AssetKind,
    pub state: ResolveState,
    /// Required key whose value may be `null` (schema: required, nullable).
    #[serde(deserialize_with = "nullable")]
    pub sha256: Option<String>,
    pub mime_hint: MimeHint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AssetKind {
    Raster,
    Vector,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MimeHint {
    #[serde(rename = "image/png")]
    Png,
    #[serde(rename = "image/jpeg")]
    Jpeg,
    #[serde(rename = "image/webp")]
    Webp,
    #[serde(rename = "image/svg+xml")]
    Svg,
}

impl MimeHint {
    /// File extension used in the content-addressed resource store.
    pub fn extension(self) -> &'static str {
        match self {
            MimeHint::Png => "png",
            MimeHint::Jpeg => "jpg",
            MimeHint::Webp => "webp",
            MimeHint::Svg => "svg",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextSpec {
    pub value: String,
    pub font_id: String,
    pub font_size: f64,
    pub line_height: f64,
    pub letter_spacing: f64,
    pub max_lines: u32,
    pub overflow_policy: OverflowPolicy,
    pub allowed_font_sizes: Vec<f64>,
    pub language: Language,
    pub align: Align,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<FontFeature>,
    #[serde(default, skip_serializing_if = "KinsokuPolicy::is_unicode")]
    pub kinsoku: KinsokuPolicy,
    /// Style ranges use UTF-8 byte offsets at character boundaries. Uncovered
    /// ranges use the base style, and author text is never rewritten.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runs: Vec<TextRun>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KinsokuPolicy {
    #[default]
    Unicode,
    StrictJapanese,
}
impl KinsokuPolicy {
    pub fn is_unicode(&self) -> bool {
        *self == Self::Unicode
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextRun {
    pub start: usize,
    pub end: usize,
    pub font_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontFeature {
    pub tag: String,
    pub value: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PathRule {
    #[default]
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stroke {
    pub color: String,
    pub width: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum NodeEffect {
    Blur {
        radius: f64,
    },
    #[serde(rename_all = "camelCase")]
    Shadow {
        color: String,
        opacity: f64,
        offset_x: f64,
        offset_y: f64,
        blur: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathClip {
    pub data: String,
    pub fill_rule: PathRule,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PathMask {
    pub data: String,
    pub fill_rule: PathRule,
    pub opacity: f64,
    pub invert: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageAdjustments {
    pub brightness: f64,
    pub contrast: f64,
    pub saturation: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodeStyle {
    pub node_id: String,
    #[serde(default)]
    pub blend_mode: BlendMode,
    /// SVG affine convention: x'=a*x+c*y+e, y'=b*x+d*y+f.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affine: Option<[f64; 6]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    /// Effects apply in the saved order. The whole node/group is filtered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<NodeEffect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<PathClip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<PathMask>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_adjustments: Option<ImageAdjustments>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected_region: Option<NormalizedRect>,
    /// Author-selected layers which must stay outside this logo's clear space.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub protected_from: Vec<String>,
}

impl NodeStyle {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            blend_mode: BlendMode::Normal,
            affine: None,
            stroke: None,
            effects: vec![],
            clip: None,
            mask: None,
            image_adjustments: None,
            protected_region: None,
            protected_from: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverflowPolicy {
    Error,
    FitApprovedSizes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Language {
    Ja,
    En,
}

impl Language {
    pub fn bcp47(self) -> &'static str {
        match self {
            Language::Ja => "ja",
            Language::En => "en",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FocalPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageFit {
    Cover,
    Contain,
    Stretch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ContainOnly {
    Contain,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Node {
    #[serde(rename_all = "camelCase")]
    Group {
        id: String,
        name: String,
        bounds: Bounds,
        rotation_deg: f64,
        opacity: f64,
        visible: bool,
        locked: bool,
        child_ids: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Rect {
        id: String,
        name: String,
        bounds: Bounds,
        rotation_deg: f64,
        opacity: f64,
        visible: bool,
        locked: bool,
        fill: Paint,
        corner_radius: f64,
    },
    #[serde(rename_all = "camelCase")]
    Path {
        id: String,
        name: String,
        bounds: Bounds,
        rotation_deg: f64,
        opacity: f64,
        visible: bool,
        locked: bool,
        data: String,
        view_box: Bounds,
        fill: Paint,
        fill_rule: PathRule,
    },
    #[serde(rename_all = "camelCase")]
    Text {
        id: String,
        name: String,
        bounds: Bounds,
        rotation_deg: f64,
        opacity: f64,
        visible: bool,
        locked: bool,
        text: TextSpec,
        fill: SolidFill,
        /// 0.3.0-core: outline (フチ) drawn behind the glyph fill.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        outline: Option<TextOutline>,
        /// 0.3.0-core: shadow of the glyphs (including the outline).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        shadow: Option<TextShadow>,
    },
    #[serde(rename_all = "camelCase")]
    Image {
        id: String,
        name: String,
        bounds: Bounds,
        rotation_deg: f64,
        opacity: f64,
        visible: bool,
        locked: bool,
        asset_id: String,
        fit: ImageFit,
        focal_point: FocalPoint,
        /// Schema: `const true`. Checked in validate.rs.
        clip_to_bounds: bool,
    },
    #[serde(rename_all = "camelCase")]
    Logo {
        id: String,
        name: String,
        bounds: Bounds,
        rotation_deg: f64,
        opacity: f64,
        visible: bool,
        locked: bool,
        asset_id: String,
        fit: ContainOnly,
        /// Schema: `const true`. Checked in validate.rs.
        preserve_aspect_ratio: bool,
        min_width: f64,
        clear_space: f64,
    },
}

impl Node {
    pub fn id(&self) -> &str {
        match self {
            Node::Group { id, .. }
            | Node::Rect { id, .. }
            | Node::Text { id, .. }
            | Node::Image { id, .. }
            | Node::Logo { id, .. }
            | Node::Path { id, .. } => id,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Node::Group { .. } => "group",
            Node::Rect { .. } => "rect",
            Node::Text { .. } => "text",
            Node::Image { .. } => "image",
            Node::Logo { .. } => "logo",
            Node::Path { .. } => "path",
        }
    }

    pub fn common(&self) -> (&Bounds, f64, f64, bool) {
        match self {
            Node::Group {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            }
            | Node::Rect {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            }
            | Node::Text {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            }
            | Node::Image {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            }
            | Node::Path {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            }
            | Node::Logo {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            } => (bounds, *rotation_deg, *opacity, *visible),
        }
    }

    /// Mutable access to the fields every node type shares.
    pub fn common_mut(&mut self) -> CommonMut<'_> {
        match self {
            Node::Group {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
                ..
            }
            | Node::Rect {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
                ..
            }
            | Node::Text {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
                ..
            }
            | Node::Image {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
                ..
            }
            | Node::Path {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
                ..
            }
            | Node::Logo {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
                ..
            } => CommonMut {
                bounds,
                rotation_deg,
                opacity,
                visible,
                locked,
            },
        }
    }

    pub fn is_locked(&self) -> bool {
        match self {
            Node::Group { locked, .. }
            | Node::Rect { locked, .. }
            | Node::Text { locked, .. }
            | Node::Image { locked, .. }
            | Node::Logo { locked, .. }
            | Node::Path { locked, .. } => *locked,
        }
    }

    pub fn bounds_mut(&mut self) -> &mut Bounds {
        match self {
            Node::Group { bounds, .. }
            | Node::Rect { bounds, .. }
            | Node::Text { bounds, .. }
            | Node::Image { bounds, .. }
            | Node::Logo { bounds, .. }
            | Node::Path { bounds, .. } => bounds,
        }
    }
}

pub struct CommonMut<'a> {
    pub bounds: &'a mut Bounds,
    pub rotation_deg: &'a mut f64,
    pub opacity: &'a mut f64,
    pub visible: &'a mut bool,
    pub locked: &'a mut bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Slot {
    pub id: String,
    #[serde(rename = "type")]
    pub slot_type: SlotType,
    pub required: bool,
    pub node_id: String,
    pub target: SlotTarget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SlotType {
    Text,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotTarget {
    #[serde(rename = "text.value")]
    TextValue,
    #[serde(rename = "assetId")]
    AssetId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Variant {
    pub id: String,
    pub label: String,
    pub width: f64,
    pub height: f64,
    pub approval_status: ApprovalStatus,
    pub node_patches: Vec<NodePatch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalStatus {
    Draft,
    Approved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NodePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affine: Option<[f64; 6]>,
    pub node_id: String,
    pub bounds: Bounds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    /// 0.3.0-core: line height for this variant (text nodes only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
}

/// Deserializes `Option<T>` while keeping the key mandatory: serde treats a
/// field with `deserialize_with` and no `default` as required.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
