//! Rust types mirroring `schemas/design.core.schema.json` (0.2.0-core).
//!
//! Every struct rejects unknown fields. Range and pattern rules from the
//! schema that serde cannot express are enforced in `validate.rs`.

use serde::{Deserialize, Deserializer, Serialize};

pub const FORMAT: &str = "siliq-creative-generator-core";
pub const SCHEMA_VERSION: &str = "0.2.0-core";

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
            Node::Group { id, .. } | Node::Rect { id, .. } | Node::Text { id, .. } | Node::Image { id, .. } | Node::Logo { id, .. } => id,
        }
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            Node::Group { .. } => "group",
            Node::Rect { .. } => "rect",
            Node::Text { .. } => "text",
            Node::Image { .. } => "image",
            Node::Logo { .. } => "logo",
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
            | Node::Logo {
                bounds,
                rotation_deg,
                opacity,
                visible,
                ..
            } => (bounds, *rotation_deg, *opacity, *visible),
        }
    }

    pub fn bounds_mut(&mut self) -> &mut Bounds {
        match self {
            Node::Group { bounds, .. }
            | Node::Rect { bounds, .. }
            | Node::Text { bounds, .. }
            | Node::Image { bounds, .. }
            | Node::Logo { bounds, .. } => bounds,
        }
    }
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
    pub node_id: String,
    pub bounds: Bounds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
}

/// Deserializes `Option<T>` while keeping the key mandatory: serde treats a
/// field with `deserialize_with` and no `default` as required.
fn nullable<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    Option::<T>::deserialize(d)
}
