//! Unified issue format (docs/05_DATA_MODEL_AND_CONTRACTS.md §9).

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// Issue codes. The `E_*` names follow docs/05 §9. Codes marked "Phase 0
/// addition" are not in that list yet and are recorded in docs/adr/0003.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Code {
    /// JSON syntax, unknown property, wrong type or out-of-range value.
    #[serde(rename = "E_SCHEMA")]
    Schema,
    /// Phase 0 addition: referential / structural rule violated.
    #[serde(rename = "E_DOCUMENT_INVALID")]
    DocumentInvalid,
    #[serde(rename = "E_FONT_MISSING")]
    FontMissing,
    #[serde(rename = "E_GLYPH_MISSING")]
    GlyphMissing,
    #[serde(rename = "E_TEXT_OVERFLOW")]
    TextOverflow,
    #[serde(rename = "E_ASSET_MISSING")]
    AssetMissing,
    /// Phase 0 addition: bytes do not match the recorded SHA-256.
    #[serde(rename = "E_ASSET_CHECKSUM")]
    AssetChecksum,
    #[serde(rename = "E_ASSET_TYPE")]
    AssetType,
    #[serde(rename = "E_SVG_UNSUPPORTED")]
    SvgUnsupported,
    #[serde(rename = "E_EXTERNAL_REFERENCE")]
    ExternalReference,
    #[serde(rename = "E_LAYOUT_UNDEFINED")]
    LayoutUndefined,
    /// Phase 0 addition: variant exists but is not approved for export.
    #[serde(rename = "E_VARIANT_NOT_APPROVED")]
    VariantNotApproved,
    /// Phase 0 addition: rendered logo narrower than its minWidth.
    #[serde(rename = "E_LOGO_MIN_WIDTH")]
    LogoMinWidth,
    #[serde(rename = "E_EXPORT_UNSUPPORTED")]
    ExportUnsupported,
    #[serde(rename = "E_OUTPUT_COLLISION")]
    OutputCollision,
    #[serde(rename = "E_PATH_DENIED")]
    PathDenied,
    #[serde(rename = "E_RESOURCE_LIMIT")]
    ResourceLimit,
    /// Phase 0 addition (warning): image drawn larger than its pixel size.
    #[serde(rename = "W_IMAGE_UPSCALED")]
    ImageUpscaled,
    /// Batch data (CSV) is malformed, or a row does not fit the template's slots.
    #[serde(rename = "E_DATA_INVALID")]
    DataInvalid,
    /// Phase 0 addition: the render engine itself failed (Skia returned null).
    #[serde(rename = "E_RENDER_FAILED")]
    RenderFailed,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::Schema => "E_SCHEMA",
            Code::DocumentInvalid => "E_DOCUMENT_INVALID",
            Code::FontMissing => "E_FONT_MISSING",
            Code::GlyphMissing => "E_GLYPH_MISSING",
            Code::TextOverflow => "E_TEXT_OVERFLOW",
            Code::AssetMissing => "E_ASSET_MISSING",
            Code::AssetChecksum => "E_ASSET_CHECKSUM",
            Code::AssetType => "E_ASSET_TYPE",
            Code::SvgUnsupported => "E_SVG_UNSUPPORTED",
            Code::ExternalReference => "E_EXTERNAL_REFERENCE",
            Code::LayoutUndefined => "E_LAYOUT_UNDEFINED",
            Code::VariantNotApproved => "E_VARIANT_NOT_APPROVED",
            Code::LogoMinWidth => "E_LOGO_MIN_WIDTH",
            Code::ExportUnsupported => "E_EXPORT_UNSUPPORTED",
            Code::OutputCollision => "E_OUTPUT_COLLISION",
            Code::PathDenied => "E_PATH_DENIED",
            Code::ResourceLimit => "E_RESOURCE_LIMIT",
            Code::ImageUpscaled => "W_IMAGE_UPSCALED",
            Code::DataInvalid => "E_DATA_INVALID",
            Code::RenderFailed => "E_RENDER_FAILED",
        }
    }
}

/// Where an issue applies. Never contains absolute file paths or text content.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Location {
    /// Batch data row (the row's `rowId`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub row_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub asset_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Issue {
    pub severity: Severity,
    pub code: Code,
    pub message: String,
    pub location: Location,
    pub remedy: String,
}

impl Issue {
    pub fn warning(code: Code, message: impl Into<String>, remedy: impl Into<String>) -> Self {
        Issue {
            severity: Severity::Warning,
            ..Issue::error(code, message, remedy)
        }
    }

    pub fn error(code: Code, message: impl Into<String>, remedy: impl Into<String>) -> Self {
        Issue {
            severity: Severity::Error,
            code,
            message: message.into(),
            location: Location::default(),
            remedy: remedy.into(),
        }
    }

    pub fn node(mut self, id: &str) -> Self {
        self.location.node_id = Some(id.to_owned());
        self
    }

    pub fn variant(mut self, id: &str) -> Self {
        self.location.variant_id = Some(id.to_owned());
        self
    }

    pub fn asset(mut self, id: &str) -> Self {
        self.location.asset_id = Some(id.to_owned());
        self
    }

    pub fn font(mut self, id: &str) -> Self {
        self.location.font_id = Some(id.to_owned());
        self
    }

    pub fn row(mut self, id: &str) -> Self {
        self.location.row_id = Some(id.to_owned());
        self
    }
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)?;
        let l = &self.location;
        for (k, v) in [
            ("row", &l.row_id),
            ("node", &l.node_id),
            ("variant", &l.variant_id),
            ("asset", &l.asset_id),
            ("font", &l.font_id),
        ] {
            if let Some(v) = v {
                write!(f, " [{k}={v}]")?;
            }
        }
        write!(f, " — remedy: {}", self.remedy)
    }
}

/// Convenience: a list of blocking issues. Rendering never proceeds when this
/// is non-empty.
pub type Issues = Vec<Issue>;

pub fn has_errors(issues: &[Issue]) -> bool {
    issues.iter().any(|i| i.severity == Severity::Error)
}
