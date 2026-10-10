//! Safe SVG subset → vector scene (docs/03_SECURITY_AND_OFFLINE.md §4).
//!
//! The input is parsed with roxmltree (DTDs rejected) and converted into an
//! engine-owned representation. Every element and attribute must be on the
//! allowlist below; anything else is an error, never silently dropped.
//! External references are reported as [`SvgErrorKind::ExternalReference`].
//!
//! Supported: `svg`, `g`, `defs`, `title`, `desc`, `path` (no arcs), `rect`,
//! `circle`, `ellipse`, `polygon`, `polyline`, `linearGradient`, `stop`;
//! `fill` (hex / none / url(#id)), `fill-opacity`, `fill-rule`, `opacity` on
//! shapes, `transform`. Not supported: strokes, `style`/CSS, `use`, `image`,
//! `text`, filters, masks, clip paths, radial gradients, arcs (`A`).

mod parse;
mod path;
mod transform;

pub use transform::Affine;

pub const MAX_SVG_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_ELEMENTS: usize = 10_000;
pub const MAX_DEPTH: usize = 32;
pub const MAX_SEGMENTS: usize = 200_000;
pub const MAX_ABS_NUMBER: f64 = 1.0e7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SvgErrorKind {
    /// Element, attribute or value outside the supported subset.
    Unsupported,
    /// href / url() pointing outside the document.
    ExternalReference,
    /// Size, depth or count limit exceeded.
    ResourceLimit,
    /// Malformed XML or SVG.
    Invalid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SvgError {
    pub kind: SvgErrorKind,
    pub message: String,
}

impl SvgError {
    pub(crate) fn new(kind: SvgErrorKind, message: impl Into<String>) -> Self {
        SvgError {
            kind,
            message: message.into(),
        }
    }
    pub(crate) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(SvgErrorKind::Unsupported, message)
    }
    pub(crate) fn invalid(message: impl Into<String>) -> Self {
        Self::new(SvgErrorKind::Invalid, message)
    }
}

impl std::fmt::Display for SvgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

/// Absolute path segments in the SVG's user space (transforms applied).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    MoveTo(Point),
    LineTo(Point),
    QuadTo(Point, Point),
    CubicTo(Point, Point, Point),
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    NonZero,
    EvenOdd,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GradientStop {
    pub offset: f64,
    pub rgb: [u8; 3],
    pub opacity: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    Solid {
        rgb: [u8; 3],
    },
    /// Linear gradient from `start` to `end`, both expressed in the space
    /// mapped to user space by `matrix` (pad spread only).
    Linear {
        start: Point,
        end: Point,
        matrix: Affine,
        stops: Vec<GradientStop>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub segments: Vec<Segment>,
    pub fill: Fill,
    pub fill_rule: FillRule,
    /// fill-opacity × opacity of the shape (group opacity is not supported).
    pub opacity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBox {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VectorArt {
    pub view_box: ViewBox,
    pub shapes: Vec<Shape>,
}

/// Convert untrusted SVG bytes into a [`VectorArt`], or fail.
pub fn import_svg(bytes: &[u8]) -> Result<VectorArt, SvgError> {
    parse::import(bytes)
}
