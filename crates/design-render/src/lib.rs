//! SILIQ Creative Generator — Phase 0 CPU render path.
//!
//! document + variant + resources
//!   → parse / validate → resolve variant → approval check
//!   → load fonts and assets (all failures collected)
//!   → resolve geometry, lay out text (approved fit sizes only)
//!   → immutable RenderPlan → Skia CPU raster → PNG
//!
//! The plan is fully resolved before drawing starts; drawing never measures
//! text or makes layout decisions.

pub mod raster;
pub mod resources;
pub mod text;

use design_model::{
    AssetKind, BlendMode, Code, DesignDocument, Issue, Issues, MimeHint, Node, NodeEffect, NodeStyle, Paint, PathRule, ResolveState,
    Stroke, has_errors, load_document, require_approved, resolve_variant,
};
use design_svg::{Fill, FillRule, Segment, VectorArt};
use resources::{MAX_IMAGE_BYTES, ResourceStore, sha256_hex};
use serde::Serialize;
use skia_safe::canvas::SaveLayerRec;
use skia_safe::gradient::{self, Gradient, Interpolation, interpolation};
use skia_safe::textlayout::Paragraph;
use skia_safe::{
    AlphaType, Canvas, ClipOp, Color, Color4f, ColorSpace, ColorType, FilterMode, Image, ImageFilter, ImageInfo, Matrix, MipmapMode,
    Paint as SkPaint, PaintStyle, Path, PathBuilder, PathFillType, RRect, Rect, SamplingOptions, Shader, TileMode, image_filters,
    png_encoder, shaders, surfaces,
};
use std::collections::HashMap;
use text::{FontBackend, FontSet, TextReport, layout_text};

pub const MAX_OUTPUT_SIDE: f64 = 16_384.0;
pub const MAX_OUTPUT_PIXELS: f64 = 64.0 * 1024.0 * 1024.0;

/// Fixed raster profile for Phase 0 (recorded in every report).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderProfile {
    pub engine: &'static str,
    pub build_fingerprint: &'static str,
    pub skia_bindings: &'static str,
    pub surface: &'static str,
    pub color_space: &'static str,
    pub anti_alias: bool,
    pub image_sampling: &'static str,
    pub gradient_interpolation: &'static str,
    pub text: &'static str,
    pub png_encoder: &'static str,
}

pub const RENDER_PROFILE: RenderProfile = RenderProfile {
    engine: "siliq design-render 0.4-core (CPU raster)",
    build_fingerprint: env!("SILIQ_RENDER_BUILD_FINGERPRINT"),
    skia_bindings: "0.153.3",
    surface: "RGBA8888, premultiplied alpha, transparent initial pixels",
    color_space: "sRGB",
    anti_alias: true,
    image_sampling: "linear filter, linear mipmaps",
    gradient_interpolation: "unpremultiplied, destination (sRGB) space, clamp",
    text: "SkParagraph, hinting off, rounding hack off, forced strut, half leading, no fallback",
    png_encoder: "Skia libpng, all filters, zlib level 6, no text chunks",
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontUse {
    pub id: String,
    pub sha256: String,
    pub face_index: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetUse {
    pub id: String,
    pub sha256: String,
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raster: Option<raster::DecodedInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderReport {
    pub document_id: String,
    pub revision: u64,
    pub variant_id: String,
    pub width: u32,
    pub height: u32,
    pub render_profile: RenderProfile,
    /// Font manager that parsed the font bytes; `null` when no text is drawn.
    pub font_backend: Option<FontBackend>,
    pub fonts: Vec<FontUse>,
    pub assets: Vec<AssetUse>,
    pub text: Vec<TextReport>,
    /// Non-blocking findings (e.g. upscaled images).
    pub warnings: Vec<Issue>,
    pub output_sha256: String,
}

pub struct RenderOutput {
    pub png: Vec<u8>,
    /// Straight RGBA8 / sRGB pixels. Row stride is width * 4.
    pub rgba: Vec<u8>,
    pub report: RenderReport,
}

// ---------------------------------------------------------------- plan --

enum Op {
    Push {
        matrix: Matrix,
        opacity: f32,
        blend: skia_safe::BlendMode,
        filter: Option<ImageFilter>,
        clip: Option<Path>,
        isolate: bool,
    },
    Pop,
    Mask {
        path: Path,
        opacity: f32,
        invert: bool,
    },
    Path {
        path: Path,
        fill: PaintSpec,
        stroke: Option<Stroke>,
    },
    Rect {
        rect: Rect,
        radius: f32,
        fill: PaintSpec,
        stroke: Option<Stroke>,
    },
    Image {
        image: Image,
        dst: Rect,
        clip: Option<Rect>,
        color_filter: Option<skia_safe::ColorFilter>,
    },
    /// Paragraph passes in paint order (shadow, outline, fill), each with its offset.
    Text {
        layers: Vec<(Paragraph, f32, f32)>,
        x: f32,
        y: f32,
    },
    Vector {
        art: VectorArt,
        matrix: Matrix,
    },
}

enum PaintSpec {
    Radial {
        center: (f32, f32),
        radius: f32,
        colors: Vec<Color4f>,
        pos: Vec<f32>,
    },
    Solid(Color4f),
    Linear {
        start: (f32, f32),
        end: (f32, f32),
        colors: Vec<Color4f>,
        pos: Vec<f32>,
    },
}

enum Prepared {
    Image(Image),
    Vector(VectorArt),
}

/// Engine options. Defaults are the production settings.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    pub font_backend: FontBackend,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            font_backend: FontBackend::preferred(),
        }
    }
}

/// Render one variant of a document to PNG.
pub fn render(doc_bytes: &[u8], variant_id: &str, store: &ResourceStore) -> Result<RenderOutput, Issues> {
    render_with(doc_bytes, variant_id, store, RenderOptions::default())
}

pub fn render_with(doc_bytes: &[u8], variant_id: &str, store: &ResourceStore, options: RenderOptions) -> Result<RenderOutput, Issues> {
    let doc = load_document(doc_bytes)?;
    render_validated(&doc, variant_id, store, options, true)
}

/// Render an in-memory document (e.g. a template with a data row bound).
/// The document is validated again before anything is drawn.
pub fn render_document(
    doc: &DesignDocument,
    variant_id: &str,
    store: &ResourceStore,
    options: RenderOptions,
) -> Result<RenderOutput, Issues> {
    let issues = design_model::validate(doc);
    if has_errors(&issues) {
        return Err(issues);
    }
    render_validated(doc, variant_id, store, options, true)
}

/// Native authoring preview uses the same CPU renderer but permits draft variants.
/// Production export functions continue to require explicit author approval.
pub fn render_preview_document(
    doc: &DesignDocument,
    variant_id: &str,
    store: &ResourceStore,
    options: RenderOptions,
) -> Result<RenderOutput, Issues> {
    let issues = design_model::validate(doc);
    if has_errors(&issues) {
        return Err(issues);
    }
    render_validated(doc, variant_id, store, options, false)
}

fn render_validated(
    doc: &DesignDocument,
    variant_id: &str,
    store: &ResourceStore,
    options: RenderOptions,
    approved_only: bool,
) -> Result<RenderOutput, Issues> {
    let variant = resolve_variant(doc, variant_id).map_err(|i| vec![i])?;
    if approved_only {
        require_approved(&variant).map_err(|i| vec![i])?;
    }

    let (w, h) = (variant.width, variant.height);
    if w.fract() != 0.0 || h.fract() != 0.0 {
        return Err(vec![
            Issue::error(
                Code::ExportUnsupported,
                format!("variant size {w}×{h} is not a whole number of pixels"),
                "Use integer artboard sizes for raster export at scale 1.",
            )
            .variant(variant_id),
        ]);
    }
    if w > MAX_OUTPUT_SIDE || h > MAX_OUTPUT_SIDE || w * h > MAX_OUTPUT_PIXELS {
        return Err(vec![
            Issue::error(
                Code::ResourceLimit,
                format!("output {w}×{h} exceeds the limit"),
                "Reduce the artboard size.",
            )
            .variant(variant_id),
        ]);
    }

    let max_effects = doc.node_styles.iter().map(|s| s.effects.len()).max().unwrap_or(0);
    if w * h * 4.0 * (3.0 + 2.0 * max_effects as f64) > 1024.0 * 1024.0 * 1024.0 {
        return Err(vec![Issue::error(
            Code::ResourceLimit,
            "render working set exceeds the 1 GiB reservation",
            "Reduce dimensions or the number of effects.",
        )]);
    }
    let nodes: HashMap<&str, &Node> = variant.nodes.iter().map(|n| (n.id(), n)).collect();
    let order = draw_order(doc, &nodes);

    let mut issues: Issues = Vec::new();
    let mut warnings: Issues = Vec::new();

    // ---- fonts used by drawn text nodes
    let mut font_ids: Vec<&str> = order
        .iter()
        .filter_map(|n| match n {
            Node::Text { text, .. } => Some(text.font_id.as_str()),
            _ => None,
        })
        .collect();
    for n in &order {
        if let Node::Text { text, .. } = n {
            font_ids.extend(text.runs.iter().map(|r| r.font_id.as_str()));
        }
    }
    font_ids.sort();
    font_ids.dedup();
    let decls: Vec<_> = doc.fonts.iter().filter(|f| font_ids.contains(&f.id.as_str())).collect();
    reserve_before_decode(doc, &nodes, &order, &variant.node_styles, &decls, store, w, h)
        .map_err(|issue| vec![issue.variant(variant_id)])?;
    let fonts = match FontSet::load_with(&decls, store, options.font_backend) {
        Ok(f) => Some(f),
        Err(mut e) => {
            issues.append(&mut e);
            None
        }
    };

    // ---- assets used by drawn image / logo nodes
    let mut prepared: HashMap<String, Prepared> = HashMap::new();
    let mut asset_uses: Vec<AssetUse> = Vec::new();
    for n in &order {
        let (Node::Image { asset_id, id, .. } | Node::Logo { asset_id, id, .. }) = n else {
            continue;
        };
        if prepared.contains_key(asset_id) || issues.iter().any(|i| i.location.asset_id.as_deref() == Some(asset_id)) {
            continue;
        }
        match prepare_asset(doc, asset_id, store) {
            Ok((p, u)) => {
                prepared.insert(asset_id.clone(), p);
                asset_uses.push(u);
            }
            Err(i) => issues.push(with_node(i.asset(asset_id), id)),
        }
    }

    let decoded_bytes: u64 = prepared
        .values()
        .filter_map(|a| match a {
            Prepared::Image(i) => Some(i.width() as u64 * i.height() as u64 * 4),
            _ => None,
        })
        .sum();
    if w * h * 4.0 * (3.0 + 2.0 * max_effects as f64) + decoded_bytes as f64 + store.snapshot_size() as f64 > 1024.0 * 1024.0 * 1024.0 {
        issues.push(Issue::error(
            Code::ResourceLimit,
            "native surfaces, decoded images and snapshot bytes exceed 1 GiB",
            "Reduce original image sizes or output dimensions.",
        ));
    }
    check_logo_clearspace(doc, &nodes, &prepared, &variant.node_styles, w, h, &mut issues);
    // ---- resolve text and geometry
    let mut text_reports = Vec::new();
    let mut ops = Vec::new();
    if let Some(fonts) = &fonts {
        build_ops(
            &doc.base_artboard.root_node_id,
            &nodes,
            fonts,
            &prepared,
            &mut ops,
            &mut text_reports,
            &mut issues,
            &mut warnings,
            &variant.node_styles,
        );
    }
    if has_errors(&issues) || fonts.is_none() {
        for i in &mut issues {
            if i.location.variant_id.is_none() {
                i.location.variant_id = Some(variant_id.to_owned());
            }
        }
        return Err(issues);
    }

    // ---- draw
    let (png, rgba) = draw(&ops, w as i32, h as i32).map_err(|i| vec![i])?;
    asset_uses.sort_by(|a, b| a.id.cmp(&b.id));
    let report = RenderReport {
        document_id: doc.document_id.clone(),
        revision: doc.revision,
        variant_id: variant_id.to_owned(),
        width: w as u32,
        height: h as u32,
        render_profile: RENDER_PROFILE,
        font_backend: (!decls.is_empty()).then_some(options.font_backend),
        fonts: fonts
            .as_ref()
            .map(|f| {
                f.loaded()
                    .map(|l| FontUse {
                        id: l.id.clone(),
                        sha256: l.sha256.clone(),
                        face_index: l.face_index,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        assets: asset_uses,
        text: text_reports,
        warnings,
        output_sha256: sha256_hex(&png),
    };
    Ok(RenderOutput { png, rgba, report })
}

#[allow(clippy::too_many_arguments)]
fn reserve_before_decode(
    doc: &DesignDocument,
    nodes: &HashMap<&str, &Node>,
    order: &[&Node],
    styles: &[NodeStyle],
    fonts: &[&design_model::Font],
    store: &ResourceStore,
    width: f64,
    height: f64,
) -> Result<(), Issue> {
    fn depth(id: &str, nodes: &HashMap<&str, &Node>, styles: &[NodeStyle], held: usize) -> usize {
        let node = nodes[id];
        if !node.common().3 {
            return held;
        }
        let style = styles.iter().find(|s| s.node_id == id);
        let effects = style.map(|s| s.effects.len()).unwrap_or(0);
        let mask = style.is_some_and(|s| s.mask.is_some());
        let layer = node.common().2 < 1.0 || effects > 0 || mask || style.is_some_and(|s| s.blend_mode != BlendMode::Normal);
        let held = held + usize::from(layer);
        let glyph_effect = matches!(node, Node::Text { shadow: Some(_), .. });
        let mut maximum = held + 2 * effects + usize::from(mask) + usize::from(glyph_effect);
        if let Node::Group { child_ids, .. } = node {
            for child in child_ids {
                maximum = maximum.max(depth(child, nodes, styles, held));
            }
        }
        maximum
    }
    let surface = (width * height * 4.0) as u64;
    let layers = depth(&doc.base_artboard.root_node_id, nodes, styles, 0);
    let mut estimated = surface.saturating_mul(3 + layers as u64) + 64 * 1024 * 1024;
    for font in fonts {
        if let Some(sha) = &font.sha256
            && let Ok((bytes, _)) = store.read("fonts", sha, &["otf", "ttf", "ttc"], resources::MAX_FONT_BYTES)
        {
            estimated = estimated.saturating_add(bytes.len() as u64 * 2);
        }
    }
    let mut assets = std::collections::HashSet::new();
    let mut largest_decode = 0u64;
    for node in order {
        let (Node::Image { id, asset_id, .. } | Node::Logo { id, asset_id, .. }) = node else {
            continue;
        };
        if !assets.insert(asset_id.as_str()) {
            continue;
        }
        let Some(asset) = doc.assets.iter().find(|a| a.id == *asset_id) else {
            continue;
        };
        let Some(sha) = &asset.sha256 else {
            continue;
        };
        let Ok((bytes, _)) = store.read("assets", sha, &[asset.mime_hint.extension()], MAX_IMAGE_BYTES) else {
            continue;
        };
        if asset.kind == AssetKind::Vector {
            estimated = estimated.saturating_add(bytes.len() as u64 * 64);
        } else {
            match raster::probe_dimensions(&bytes, asset.mime_hint) {
                Ok((w, h)) => {
                    let pixels = u64::from(w) * u64::from(h) * 4;
                    estimated = estimated.saturating_add(pixels);
                    largest_decode = largest_decode.max(pixels);
                }
                Err(issue) if issue.code == Code::ResourceLimit => return Err(issue.node(id)),
                Err(_) => {} // Normal preflight reports the malformed input, alongside other problems.
            }
        }
        if estimated > 1024 * 1024 * 1024 {
            break;
        }
    }
    estimated = estimated
        .saturating_add(store.snapshot_size())
        .saturating_add(largest_decode.saturating_mul(2));
    if estimated > 1024 * 1024 * 1024 {
        return Err(Issue::error(
            Code::ResourceLimit,
            format!(
                "estimated native working set is {} MiB; limit is 1024 MiB",
                estimated.div_ceil(1024 * 1024)
            ),
            "Reduce the number/size of source assets, output dimensions or nested effects before rendering.",
        ));
    }
    Ok(())
}

fn with_node(mut i: Issue, node: &str) -> Issue {
    if i.location.node_id.is_none() {
        i.location.node_id = Some(node.to_owned());
    }
    i
}

/// Visible nodes in paint order (depth-first through childIds).
fn draw_order<'a>(doc: &DesignDocument, nodes: &HashMap<&str, &'a Node>) -> Vec<&'a Node> {
    let mut out = Vec::new();
    let mut stack = vec![doc.base_artboard.root_node_id.as_str()];
    while let Some(id) = stack.pop() {
        let n = nodes[id];
        if !n.common().3 {
            continue;
        }
        out.push(n);
        if let Node::Group { child_ids, .. } = n {
            stack.extend(child_ids.iter().rev().map(String::as_str));
        }
    }
    out
}

fn prepare_asset(doc: &DesignDocument, asset_id: &str, store: &ResourceStore) -> Result<(Prepared, AssetUse), Issue> {
    let a = doc.assets.iter().find(|a| a.id == asset_id).expect("validated asset reference");
    let sha = match (a.state, &a.sha256) {
        (ResolveState::Resolved, Some(s)) => s.clone(),
        _ => {
            return Err(Issue::error(
                Code::AssetMissing,
                format!("asset {asset_id:?} is unresolved (no file selected)"),
                "Select the file for this asset; nothing is fetched or substituted.",
            ));
        }
    };
    let (bytes, _) = store
        .read("assets", &sha, &[a.mime_hint.extension()], MAX_IMAGE_BYTES)
        .map_err(|e| e.into_issue("asset", Code::AssetMissing))?;
    match a.kind {
        AssetKind::Vector => {
            debug_assert_eq!(a.mime_hint, MimeHint::Svg);
            let art = design_svg::import_svg(&bytes).map_err(|e| {
                let code = match e.kind {
                    design_svg::SvgErrorKind::ExternalReference => Code::ExternalReference,
                    design_svg::SvgErrorKind::ResourceLimit => Code::ResourceLimit,
                    _ => Code::SvgUnsupported,
                };
                Issue::error(
                    code,
                    format!("SVG rejected: {}", e.message),
                    "Supply artwork that uses only the supported SVG subset (see docs/adr/0002).",
                )
            })?;
            Ok((
                Prepared::Vector(art),
                AssetUse {
                    id: asset_id.to_owned(),
                    sha256: sha,
                    kind: "vector",
                    raster: None,
                },
            ))
        }
        AssetKind::Raster => {
            let (image, info) = raster::decode(&bytes, a.mime_hint)?;
            Ok((
                Prepared::Image(image),
                AssetUse {
                    id: asset_id.to_owned(),
                    sha256: sha,
                    kind: "raster",
                    raster: Some(info),
                },
            ))
        }
    }
}

fn check_logo_clearspace(
    doc: &DesignDocument,
    nodes: &HashMap<&str, &Node>,
    prepared: &HashMap<String, Prepared>,
    styles: &[NodeStyle],
    width: f64,
    height: f64,
    issues: &mut Issues,
) {
    fn geometry(
        id: &str,
        nodes: &HashMap<&str, &Node>,
        parent: &Matrix,
        styles: &[NodeStyle],
        world: &mut HashMap<String, (Matrix, Rect)>,
    ) {
        let node = nodes[id];
        let (b, rotation, _, visible) = node.common();
        if !visible {
            return;
        }
        let matrix = *parent * node_matrix(b, rotation) * affine_matrix(styles.iter().find(|s| s.node_id == id).and_then(|s| s.affine));
        let rect = Rect::from_xywh(b.x as f32, b.y as f32, b.width as f32, b.height as f32);
        let frame_matrix = if matches!(node, Node::Group { .. }) {
            *parent * node_matrix(b, rotation)
        } else {
            matrix
        };
        world.insert(id.to_owned(), (matrix, frame_matrix.map_rect(rect).0));
        if let Node::Group { child_ids, .. } = node {
            for child in child_ids {
                geometry(child, nodes, &matrix, styles, world);
            }
        }
    }
    let mut world = HashMap::new();
    geometry(&doc.base_artboard.root_node_id, nodes, &Matrix::new_identity(), styles, &mut world);
    for (id, (matrix, _)) in &world {
        if !matrix.is_finite() {
            issues.push(
                Issue::error(
                    Code::ResourceLimit,
                    "combined node transform exceeds native precision",
                    "Reduce nested scales.",
                )
                .node(id),
            );
        }
    }
    for node in nodes.values() {
        let Node::Logo {
            id,
            bounds: b,
            asset_id,
            clear_space,
            min_width,
            ..
        } = node
        else {
            continue;
        };
        let Some((matrix, _)) = world.get(id) else {
            continue;
        };
        let (vw, vh) = match prepared.get(asset_id) {
            Some(Prepared::Vector(v)) => (v.view_box.width, v.view_box.height),
            Some(Prepared::Image(i)) => (i.width() as f64, i.height() as f64),
            None => continue,
        };
        let scale = (b.width / vw).min(b.height / vh);
        let (dw, dh) = (vw * scale, vh * scale);
        let x_axis = matrix.map_vector((1.0, 0.0));
        let y_axis = matrix.map_vector((0.0, 1.0));
        let sx = f64::from(x_axis.x).hypot(f64::from(x_axis.y));
        let sy = f64::from(y_axis.x).hypot(f64::from(y_axis.y));
        let dot = f64::from(x_axis.x) * f64::from(y_axis.x) + f64::from(x_axis.y) * f64::from(y_axis.y);
        if (sx - sy).abs() > 1e-5 * sx.max(sy) || dot.abs() > 1e-5 * sx * sy {
            issues.push(
                Issue::error(
                    Code::DocumentInvalid,
                    "a parent transform changes the logo's aspect ratio",
                    "Use uniform group scaling or keep the logo outside that group.",
                )
                .node(id)
                .asset(asset_id),
            );
        }
        if dw * sx + 1e-9 < *min_width {
            issues.push(
                Issue::error(
                    Code::LogoMinWidth,
                    format!("transformed logo width {:.2} is below minimum {min_width}", dw * sx),
                    "Enlarge the authored logo or group; the logo is never distorted.",
                )
                .node(id)
                .asset(asset_id),
            );
        }
        if *clear_space == 0.0 {
            continue;
        }
        let local = Rect::from_xywh(
            (b.x + (b.width - dw) / 2.0 - clear_space) as f32,
            (b.y + (b.height - dh) / 2.0 - clear_space) as f32,
            (dw + 2.0 * clear_space) as f32,
            (dh + 2.0 * clear_space) as f32,
        );
        let protected = matrix.map_rect(local).0;
        let outside = protected.left < -0.01
            || protected.top < -0.01
            || protected.right > width as f32 + 0.01
            || protected.bottom > height as f32 + 0.01;
        let targets = styles
            .iter()
            .find(|s| s.node_id == *id)
            .map(|s| s.protected_from.as_slice())
            .unwrap_or(&[]);
        let intersects = targets.iter().find(|target| {
            world.get(*target).is_some_and(|(_, r)| {
                r.left < protected.right && r.right > protected.left && r.top < protected.bottom && r.bottom > protected.top
            })
        });
        if outside || intersects.is_some() {
            issues.push(
                Issue::error(
                    Code::LogoClearSpace,
                    if let Some(target) = intersects {
                        format!("logo clear space intersects author-protected node {target}")
                    } else {
                        "logo clear space extends outside the artboard".into()
                    },
                    "Move the specified layers or revise the author's logo clearance requirement.",
                )
                .node(id)
                .asset(asset_id),
            );
        }
    }
}

fn parse_hex(c: &str) -> [u8; 3] {
    let v = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).expect("validated color");
    [v(1), v(3), v(5)]
}

fn color4f(rgb: [u8; 3], alpha: f64) -> Color4f {
    Color4f::new(rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, alpha as f32)
}

fn affine_matrix(affine: Option<[f64; 6]>) -> Matrix {
    match affine {
        Some([a, b, c, d, e, f]) => Matrix::new_all(a as f32, c as f32, e as f32, b as f32, d as f32, f as f32, 0.0, 0.0, 1.0),
        None => Matrix::new_identity(),
    }
}

/// Rotation about the centre of the node's bounds.
fn node_matrix(b: &design_model::Bounds, rotation_deg: f64) -> Matrix {
    if rotation_deg == 0.0 {
        return Matrix::new_identity();
    }
    Matrix::rotate_deg_pivot(rotation_deg as f32, ((b.x + b.width / 2.0) as f32, (b.y + b.height / 2.0) as f32))
}

#[allow(clippy::too_many_arguments)]
fn build_ops(
    id: &str,
    nodes: &HashMap<&str, &Node>,
    fonts: &FontSet,
    prepared: &HashMap<String, Prepared>,
    ops: &mut Vec<Op>,
    text_reports: &mut Vec<TextReport>,
    issues: &mut Issues,
    warnings: &mut Issues,
    styles: &[NodeStyle],
) {
    let n = nodes[id];
    let (b, rot, opacity, visible) = n.common();
    if !visible {
        return;
    }
    let style = styles.iter().find(|s| s.node_id == id);
    let (filter, clip, mask, stroke, blend) = match style {
        Some(style) => (
            effect_filter(&style.effects),
            style.clip.as_ref().map(|c| model_path(&c.data, c.fill_rule)),
            style
                .mask
                .as_ref()
                .map(|m| (model_path(&m.data, m.fill_rule), m.opacity as f32, m.invert)),
            style.stroke.clone(),
            match style.blend_mode {
                BlendMode::Normal => skia_safe::BlendMode::SrcOver,
                BlendMode::Multiply => skia_safe::BlendMode::Multiply,
                BlendMode::Screen => skia_safe::BlendMode::Screen,
            },
        ),
        None => (None, None, None, None, skia_safe::BlendMode::SrcOver),
    };
    if let Some(style) = style
        && !style.effects.is_empty()
        && filter.is_none()
    {
        issues.push(
            Issue::error(
                Code::RenderFailed,
                "effect filter allocation failed",
                "Reduce effects or report an engine defect.",
            )
            .node(id),
        );
    }
    let has_mask = mask.is_some();
    ops.push(Op::Push {
        matrix: node_matrix(b, rot) * affine_matrix(style.and_then(|s| s.affine)),
        opacity: opacity as f32,
        blend,
        filter,
        clip,
        isolate: has_mask,
    });
    let rect = Rect::from_xywh(b.x as f32, b.y as f32, b.width as f32, b.height as f32);
    match n {
        Node::Group { child_ids, .. } => {
            for c in child_ids {
                build_ops(c, nodes, fonts, prepared, ops, text_reports, issues, warnings, styles);
            }
        }
        Node::Rect { fill, corner_radius, .. } => {
            let radius = corner_radius.min(b.width / 2.0).min(b.height / 2.0) as f32;
            ops.push(Op::Rect {
                rect,
                radius,
                fill: paint_spec(fill, b),
                stroke: stroke.clone(),
            });
        }
        Node::Path {
            data,
            view_box,
            fill,
            fill_rule,
            ..
        } => {
            let matrix = Matrix::translate((b.x as f32, b.y as f32))
                * Matrix::scale(((b.width / view_box.width) as f32, (b.height / view_box.height) as f32))
                * Matrix::translate((-view_box.x as f32, -view_box.y as f32));
            ops.push(Op::Path {
                path: model_path(data, *fill_rule).with_transform(&matrix),
                fill: paint_spec(fill, b),
                stroke: stroke.clone(),
            });
        }
        Node::Text {
            text,
            fill,
            outline,
            shadow,
            ..
        } => match layout_text(id, text, parse_hex(&fill.color), b.width, b.height, fonts)
            .and_then(|laid| text::effect_layers(id, text, &laid, b.width, outline.as_ref(), shadow.as_ref(), fonts).map(|fx| (laid, fx)))
        {
            Ok((laid, fx)) => {
                let mut layers: Vec<(Paragraph, f32, f32)> = fx.into_iter().map(|l| (l.paragraph, l.dx, l.dy)).collect();
                text_reports.push(laid.report);
                layers.push((laid.paragraph, 0.0, 0.0));
                ops.push(Op::Text {
                    layers,
                    x: b.x as f32,
                    y: b.y as f32,
                });
            }
            Err(i) => issues.push(i),
        },
        Node::Image {
            asset_id,
            fit,
            focal_point,
            clip_to_bounds,
            ..
        } => {
            if let Some(Prepared::Image(img)) = prepared.get(asset_id) {
                let (iw, ih) = (img.width() as f64, img.height() as f64);
                let (sx, sy) = match fit {
                    design_model::ImageFit::Cover => {
                        let s = (b.width / iw).max(b.height / ih);
                        (s, s)
                    }
                    design_model::ImageFit::Contain => {
                        let s = (b.width / iw).min(b.height / ih);
                        (s, s)
                    }
                    design_model::ImageFit::Stretch => (b.width / iw, b.height / ih),
                };
                let s = sx.max(sy);
                let (dw, dh) = (iw * sx, ih * sy);
                let place = |box_pos: f64, box_len: f64, len: f64, focal: f64| {
                    let lo = (box_len - len).min(0.0);
                    let hi = (box_len - len).max(0.0);
                    box_pos + (box_len / 2.0 - focal * len).clamp(lo, hi)
                };
                let dx = place(b.x, b.width, dw, focal_point.x);
                let dy = place(b.y, b.height, dh, focal_point.y);
                if let Some(region) = style.and_then(|s| s.protected_region.as_ref())
                    && *clip_to_bounds
                    && (dx + region.x * dw < b.x - 0.01
                        || dy + region.y * dh < b.y - 0.01
                        || dx + (region.x + region.width) * dw > b.x + b.width + 0.01
                        || dy + (region.y + region.height) * dh > b.y + b.height + 0.01)
                {
                    issues.push(
                        Issue::error(
                            Code::DocumentInvalid,
                            "crop removes part of the author-protected image region",
                            "Choose contain or adjust the focal point/box.",
                        )
                        .node(id)
                        .asset(asset_id),
                    );
                }
                if s > 1.0 + 1e-9 {
                    warnings.push(
                        Issue::warning(
                            Code::ImageUpscaled,
                            format!("image is upscaled ×{s:.3}"),
                            "Use a higher-resolution image for sharper output.",
                        )
                        .node(id)
                        .asset(asset_id),
                    );
                }
                ops.push(Op::Image {
                    image: img.clone(),
                    dst: Rect::from_xywh(dx as f32, dy as f32, dw as f32, dh as f32),
                    clip: clip_to_bounds.then_some(rect),
                    color_filter: style.and_then(|s| s.image_adjustments.as_ref()).map(adjustment_filter),
                });
            }
        }
        Node::Logo { asset_id, .. } => {
            let (vw, vh, vx, vy) = match prepared.get(asset_id) {
                Some(Prepared::Vector(art)) => (art.view_box.width, art.view_box.height, art.view_box.x, art.view_box.y),
                Some(Prepared::Image(img)) => (img.width() as f64, img.height() as f64, 0.0, 0.0),
                None => {
                    ops.push(Op::Pop);
                    return;
                }
            };
            // contain, aspect preserved, centred (xMidYMid meet).
            let s = (b.width / vw).min(b.height / vh);
            let (dw, dh) = (vw * s, vh * s);
            let ox = b.x + (b.width - dw) / 2.0;
            let oy = b.y + (b.height - dh) / 2.0;
            match &prepared[asset_id] {
                Prepared::Vector(art) => {
                    let m = Matrix::translate((ox as f32, oy as f32))
                        * Matrix::scale((s as f32, s as f32))
                        * Matrix::translate((-vx as f32, -vy as f32));
                    ops.push(Op::Vector {
                        art: art.clone(),
                        matrix: m,
                    });
                }
                Prepared::Image(img) => {
                    let dst = Rect::from_xywh(ox as f32, oy as f32, dw as f32, dh as f32);
                    ops.push(Op::Image {
                        image: img.clone(),
                        dst,
                        clip: Some(dst),
                        color_filter: None,
                    });
                }
            }
        }
    }
    if let Some((path, opacity, invert)) = mask {
        ops.push(Op::Mask { path, opacity, invert });
    }
    ops.push(Op::Pop);
}

fn model_path(data: &str, rule: PathRule) -> Path {
    let segments = design_svg::parse_path(data).expect("document paths already validated");
    segment_path(
        &segments,
        match rule {
            PathRule::NonZero => PathFillType::Winding,
            PathRule::EvenOdd => PathFillType::EvenOdd,
        },
    )
}

fn segment_path(segments: &[Segment], rule: PathFillType) -> Path {
    let mut pb = PathBuilder::new();
    let point = |p: design_svg::Point| (p.x as f32, p.y as f32);
    for seg in segments {
        match *seg {
            Segment::MoveTo(p) => {
                pb.move_to(point(p));
            }
            Segment::LineTo(p) => {
                pb.line_to(point(p));
            }
            Segment::QuadTo(a, b) => {
                pb.quad_to(point(a), point(b));
            }
            Segment::CubicTo(a, b, c) => {
                pb.cubic_to(point(a), point(b), point(c));
            }
            Segment::Close => {
                pb.close();
            }
        }
    }
    pb.set_fill_type(rule);
    pb.detach()
}

fn effect_filter(effects: &[NodeEffect]) -> Option<ImageFilter> {
    let mut filter = None;
    for effect in effects {
        filter = match effect {
            NodeEffect::Blur { radius } => {
                let sigma = (0.57735 * radius + 0.5) as f32;
                image_filters::blur((sigma, sigma), TileMode::Decal, filter, None)
            }
            NodeEffect::Shadow {
                color,
                opacity,
                offset_x,
                offset_y,
                blur,
            } => {
                let sigma = if *blur == 0.0 { 0.0 } else { (0.57735 * blur + 0.5) as f32 };
                image_filters::drop_shadow(
                    (*offset_x as f32, *offset_y as f32),
                    (sigma, sigma),
                    color4f(parse_hex(color), *opacity),
                    ColorSpace::new_srgb(),
                    filter,
                    None,
                )
            }
        };
    }
    filter
}

/// Gradient geometry: angle 0° runs left → right, 90° top → bottom (y down,
/// clockwise). The gradient line passes through the centre and its length is
/// chosen so that the 0 and 1 stops touch the box corners (CSS-style length).
fn paint_spec(p: &Paint, b: &design_model::Bounds) -> PaintSpec {
    match p {
        Paint::Solid { color } => PaintSpec::Solid(color4f(parse_hex(color), 1.0)),
        Paint::RadialGradient { center, radius, stops } => PaintSpec::Radial {
            center: ((b.x + center.x * b.width) as f32, (b.y + center.y * b.height) as f32),
            radius: (radius * b.width.max(b.height)) as f32,
            colors: stops.iter().map(|s| color4f(parse_hex(&s.color), s.opacity)).collect(),
            pos: stops.iter().map(|s| s.offset as f32).collect(),
        },
        Paint::LinearGradient { angle_deg, stops } => {
            let (sin, cos) = angle_deg.to_radians().sin_cos();
            let half = (b.width * cos.abs() + b.height * sin.abs()) / 2.0;
            let (cx, cy) = (b.x + b.width / 2.0, b.y + b.height / 2.0);
            PaintSpec::Linear {
                start: ((cx - cos * half) as f32, (cy - sin * half) as f32),
                end: ((cx + cos * half) as f32, (cy + sin * half) as f32),
                colors: stops.iter().map(|s| color4f(parse_hex(&s.color), s.opacity)).collect(),
                pos: stops.iter().map(|s| s.offset as f32).collect(),
            }
        }
    }
}

fn interpolation() -> Interpolation {
    Interpolation {
        in_premul: interpolation::InPremul::No,
        color_space: interpolation::ColorSpace::Destination,
        hue_method: interpolation::HueMethod::Shorter,
    }
}

fn linear_shader(start: (f32, f32), end: (f32, f32), colors: &[Color4f], pos: &[f32], local: Option<&Matrix>) -> Option<Shader> {
    let colors = gradient::Colors::new(colors, Some(pos), TileMode::Clamp, ColorSpace::new_srgb());
    shaders::linear_gradient((start, end), &Gradient::new(colors, interpolation()), local)
}

fn sk_paint(spec: &PaintSpec) -> Option<SkPaint> {
    let mut paint = SkPaint::default();
    paint.set_anti_alias(true).set_style(PaintStyle::Fill);
    match spec {
        PaintSpec::Radial {
            center,
            radius,
            colors,
            pos,
        } => {
            let colors = gradient::Colors::new(colors, Some(pos), TileMode::Clamp, ColorSpace::new_srgb());
            paint.set_shader(shaders::radial_gradient(
                (*center, *radius),
                &Gradient::new(colors, interpolation()),
                None,
            )?);
        }
        PaintSpec::Solid(c) => {
            paint.set_color4f(*c, &ColorSpace::new_srgb());
        }
        PaintSpec::Linear { start, end, colors, pos } => {
            let shader = linear_shader(*start, *end, colors, pos, None)?;
            paint.set_shader(shader);
        }
    }
    Some(paint)
}

fn draw(ops: &[Op], w: i32, h: i32) -> Result<(Vec<u8>, Vec<u8>), Issue> {
    let fail = |what: &str| {
        Issue::error(
            Code::RenderFailed,
            format!("render engine failed: {what}"),
            "Report this as an engine defect.",
        )
    };
    let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Premul, ColorSpace::new_srgb());
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| fail("surface allocation"))?;
    let canvas = surface.canvas();
    canvas.clear(Color::TRANSPARENT);
    let sampling = SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear);

    for op in ops {
        match op {
            Op::Push {
                matrix,
                opacity,
                blend,
                filter,
                clip,
                isolate,
            } => {
                if *opacity < 1.0 || *blend != skia_safe::BlendMode::SrcOver || filter.is_some() || *isolate {
                    let mut paint = SkPaint::default();
                    paint.set_alpha_f(*opacity).set_blend_mode(*blend).set_image_filter(filter.clone());
                    canvas.save_layer(&SaveLayerRec::default().paint(&paint));
                } else {
                    canvas.save();
                }
                canvas.concat(matrix);
                if let Some(path) = clip {
                    canvas.clip_path(path, ClipOp::Intersect, true);
                }
            }
            Op::Mask { path, opacity, invert } => {
                let mut composite = SkPaint::default();
                composite.set_blend_mode(if *invert {
                    skia_safe::BlendMode::DstOut
                } else {
                    skia_safe::BlendMode::DstIn
                });
                canvas.save_layer(&SaveLayerRec::default().paint(&composite));
                let mut paint = SkPaint::default();
                paint.set_color(Color::WHITE).set_alpha_f(*opacity).set_anti_alias(true);
                canvas.draw_path(path, &paint);
                canvas.restore();
            }
            Op::Path { path, fill, stroke } => {
                let paint = sk_paint(fill).ok_or_else(|| fail("gradient shader"))?;
                canvas.draw_path(path, &paint);
                if let Some(stroke) = stroke {
                    canvas.draw_path(path, &stroke_paint(stroke));
                }
            }
            Op::Pop => {
                canvas.restore();
            }
            Op::Rect {
                rect,
                radius,
                fill,
                stroke,
            } => {
                let paint = sk_paint(fill).ok_or_else(|| fail("gradient shader"))?;
                if *radius > 0.0 {
                    canvas.draw_rrect(RRect::new_rect_xy(*rect, *radius, *radius), &paint);
                } else {
                    canvas.draw_rect(*rect, &paint);
                }
                if let Some(stroke) = stroke {
                    let paint = stroke_paint(stroke);
                    if *radius > 0.0 {
                        canvas.draw_rrect(RRect::new_rect_xy(*rect, *radius, *radius), &paint);
                    } else {
                        canvas.draw_rect(*rect, &paint);
                    }
                }
            }
            Op::Image {
                image,
                dst,
                clip,
                color_filter,
            } => {
                canvas.save();
                if let Some(clip) = clip {
                    canvas.clip_rect(*clip, ClipOp::Intersect, true);
                }
                let mut paint = SkPaint::default();
                paint.set_anti_alias(true).set_color_filter(color_filter.clone());
                canvas.draw_image_rect_with_sampling_options(image, None, *dst, sampling, &paint);
                canvas.restore();
            }
            Op::Text { layers, x, y } => {
                for (paragraph, dx, dy) in layers {
                    paragraph.paint(canvas, (*x + *dx, *y + *dy));
                }
            }
            Op::Vector { art, matrix } => draw_vector(canvas, art, matrix).map_err(|_| fail("vector shader"))?,
        }
    }

    let image = surface.image_snapshot();
    let pixmap = image.peek_pixels().ok_or_else(|| fail("pixel access"))?;
    let mut png = Vec::new();
    if !png_encoder::encode(&pixmap, &mut png, &png_encoder::Options::default()) {
        return Err(fail("PNG encoding"));
    }
    let straight_info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, ColorSpace::new_srgb());
    let mut rgba = vec![0u8; w as usize * h as usize * 4];
    if !image.read_pixels(
        &straight_info,
        &mut rgba,
        w as usize * 4,
        (0, 0),
        skia_safe::image::CachingHint::Disallow,
    ) {
        return Err(fail("straight RGBA readback"));
    }
    Ok((png, rgba))
}

fn adjustment_filter(adjustments: &design_model::ImageAdjustments) -> skia_safe::ColorFilter {
    // Explicit encoded-sRGB channel adjustment: saturation, then contrast about
    // 0.5, then brightness. Alpha is unchanged; no inferred enhancement.
    let sat = adjustments.saturation as f32;
    let con = adjustments.contrast as f32;
    let lum = [0.2126f32, 0.7152, 0.0722];
    let mut matrix = [0f32; 20];
    for row in 0..3 {
        for column in 0..3 {
            matrix[row * 5 + column] = con * ((1.0 - sat) * lum[column] + if row == column { sat } else { 0.0 });
        }
        matrix[row * 5 + 4] = 0.5 * (1.0 - con) + adjustments.brightness as f32;
    }
    matrix[18] = 1.0;
    skia_safe::color_filters::matrix_row_major(&matrix, None)
}

fn stroke_paint(stroke: &Stroke) -> SkPaint {
    let mut paint = SkPaint::default();
    paint
        .set_anti_alias(true)
        .set_style(PaintStyle::Stroke)
        .set_stroke_width(stroke.width as f32)
        .set_color4f(color4f(parse_hex(&stroke.color), 1.0), &ColorSpace::new_srgb());
    paint
}

fn draw_vector(canvas: &Canvas, art: &VectorArt, matrix: &Matrix) -> Result<(), ()> {
    canvas.save();
    canvas.concat(matrix);
    // The SVG viewport clips its content to the viewBox.
    let vb = art.view_box;
    canvas.clip_rect(
        Rect::from_xywh(vb.x as f32, vb.y as f32, vb.width as f32, vb.height as f32),
        ClipOp::Intersect,
        true,
    );
    for shape in &art.shapes {
        let mut pb = PathBuilder::new();
        for seg in &shape.segments {
            let p = |q: design_svg::Point| (q.x as f32, q.y as f32);
            match *seg {
                Segment::MoveTo(a) => {
                    pb.move_to(p(a));
                }
                Segment::LineTo(a) => {
                    pb.line_to(p(a));
                }
                Segment::QuadTo(a, b) => {
                    pb.quad_to(p(a), p(b));
                }
                Segment::CubicTo(a, b, c) => {
                    pb.cubic_to(p(a), p(b), p(c));
                }
                Segment::Close => {
                    pb.close();
                }
            }
        }
        pb.set_fill_type(match shape.fill_rule {
            FillRule::NonZero => PathFillType::Winding,
            FillRule::EvenOdd => PathFillType::EvenOdd,
        });
        let path = pb.detach();
        let mut paint = SkPaint::default();
        paint.set_anti_alias(true).set_style(PaintStyle::Fill);
        match &shape.fill {
            Fill::Solid { rgb } => {
                paint.set_color4f(color4f(*rgb, shape.opacity), &ColorSpace::new_srgb());
            }
            Fill::Linear {
                start,
                end,
                matrix: m,
                stops,
            } => {
                let colors: Vec<Color4f> = stops.iter().map(|s| color4f(s.rgb, s.opacity)).collect();
                let pos: Vec<f32> = stops.iter().map(|s| s.offset as f32).collect();
                let local = Matrix::new_all(
                    m.a as f32, m.c as f32, m.e as f32, m.b as f32, m.d as f32, m.f as f32, 0.0, 0.0, 1.0,
                );
                let shader = linear_shader(
                    (start.x as f32, start.y as f32),
                    (end.x as f32, end.y as f32),
                    &colors,
                    &pos,
                    Some(&local),
                )
                .ok_or(())?;
                paint.set_shader(shader);
                paint.set_alpha_f(shape.opacity as f32);
            }
        }
        canvas.draw_path(&path, &paint);
    }
    canvas.restore();
    Ok(())
}
