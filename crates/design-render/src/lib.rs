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
    AssetKind, Code, DesignDocument, Issue, Issues, MimeHint, Node, Paint, ResolveState, has_errors, load_document, require_approved,
    resolve_variant,
};
use design_svg::{Fill, FillRule, Segment, VectorArt};
use resources::{MAX_IMAGE_BYTES, ResourceStore, sha256_hex};
use serde::Serialize;
use skia_safe::gradient::{self, Gradient, Interpolation, interpolation};
use skia_safe::textlayout::Paragraph;
use skia_safe::{
    AlphaType, Canvas, ClipOp, Color, Color4f, ColorSpace, ColorType, FilterMode, Image, ImageInfo, Matrix, MipmapMode, Paint as SkPaint,
    PaintStyle, PathBuilder, PathFillType, RRect, Rect, SamplingOptions, Shader, TileMode, png_encoder, shaders, surfaces,
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
    engine: "siliq design-render 0.0.1 (CPU raster)",
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
    pub report: RenderReport,
}

// ---------------------------------------------------------------- plan --

enum Op {
    Push { matrix: Matrix, opacity: f32 },
    Pop,
    Rect { rect: Rect, radius: f32, fill: PaintSpec },
    Image { image: Image, dst: Rect, clip: Rect },
    Text { paragraph: Paragraph, x: f32, y: f32 },
    Vector { art: VectorArt, matrix: Matrix },
}

enum PaintSpec {
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
    let variant = resolve_variant(&doc, variant_id).map_err(|i| vec![i])?;
    require_approved(&variant).map_err(|i| vec![i])?;

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

    let nodes: HashMap<&str, &Node> = variant.nodes.iter().map(|n| (n.id(), n)).collect();
    let order = draw_order(&doc, &nodes);

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
    font_ids.sort();
    font_ids.dedup();
    let decls: Vec<_> = doc.fonts.iter().filter(|f| font_ids.contains(&f.id.as_str())).collect();
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
        match prepare_asset(&doc, asset_id, store) {
            Ok((p, u)) => {
                prepared.insert(asset_id.clone(), p);
                asset_uses.push(u);
            }
            Err(i) => issues.push(with_node(i.asset(asset_id), id)),
        }
    }

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
    let png = draw(&ops, w as i32, h as i32).map_err(|i| vec![i])?;
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
    Ok(RenderOutput { png, report })
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

fn parse_hex(c: &str) -> [u8; 3] {
    let v = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).expect("validated color");
    [v(1), v(3), v(5)]
}

fn color4f(rgb: [u8; 3], alpha: f64) -> Color4f {
    Color4f::new(rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, alpha as f32)
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
) {
    let n = nodes[id];
    let (b, rot, opacity, visible) = n.common();
    if !visible {
        return;
    }
    ops.push(Op::Push {
        matrix: node_matrix(b, rot),
        opacity: opacity as f32,
    });
    let rect = Rect::from_xywh(b.x as f32, b.y as f32, b.width as f32, b.height as f32);
    match n {
        Node::Group { child_ids, .. } => {
            for c in child_ids {
                build_ops(c, nodes, fonts, prepared, ops, text_reports, issues, warnings);
            }
        }
        Node::Rect { fill, corner_radius, .. } => {
            let radius = corner_radius.min(b.width / 2.0).min(b.height / 2.0) as f32;
            ops.push(Op::Rect {
                rect,
                radius,
                fill: paint_spec(fill, b),
            });
        }
        Node::Text { text, fill, .. } => match layout_text(id, text, parse_hex(&fill.color), b.width, b.height, fonts) {
            Ok(laid) => {
                text_reports.push(laid.report);
                ops.push(Op::Text {
                    paragraph: laid.paragraph,
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
            ..
        } => {
            if let Some(Prepared::Image(img)) = prepared.get(asset_id) {
                let (iw, ih) = (img.width() as f64, img.height() as f64);
                let s = match fit {
                    design_model::ImageFit::Cover => (b.width / iw).max(b.height / ih),
                    design_model::ImageFit::Contain => (b.width / iw).min(b.height / ih),
                };
                let (dw, dh) = (iw * s, ih * s);
                let place = |box_pos: f64, box_len: f64, len: f64, focal: f64| {
                    let lo = (box_len - len).min(0.0);
                    let hi = (box_len - len).max(0.0);
                    box_pos + (box_len / 2.0 - focal * len).clamp(lo, hi)
                };
                let dx = place(b.x, b.width, dw, focal_point.x);
                let dy = place(b.y, b.height, dh, focal_point.y);
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
                    clip: rect,
                });
            }
        }
        Node::Logo { asset_id, min_width, .. } => {
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
            if dw + 1e-9 < *min_width {
                issues.push(
                    Issue::error(
                        Code::LogoMinWidth,
                        format!("logo would be drawn {dw:.2} du wide; minimum is {min_width}"),
                        "Have the author enlarge the logo box; the logo is never distorted to fit.",
                    )
                    .node(id)
                    .asset(asset_id),
                );
            }
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
                        clip: dst,
                    });
                }
            }
        }
    }
    ops.push(Op::Pop);
}

/// Gradient geometry: angle 0° runs left → right, 90° top → bottom (y down,
/// clockwise). The gradient line passes through the centre and its length is
/// chosen so that the 0 and 1 stops touch the box corners (CSS-style length).
fn paint_spec(p: &Paint, b: &design_model::Bounds) -> PaintSpec {
    match p {
        Paint::Solid { color } => PaintSpec::Solid(color4f(parse_hex(color), 1.0)),
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

fn draw(ops: &[Op], w: i32, h: i32) -> Result<Vec<u8>, Issue> {
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
            Op::Push { matrix, opacity } => {
                if *opacity < 1.0 {
                    canvas.save_layer_alpha_f(None, *opacity);
                } else {
                    canvas.save();
                }
                canvas.concat(matrix);
            }
            Op::Pop => {
                canvas.restore();
            }
            Op::Rect { rect, radius, fill } => {
                let paint = sk_paint(fill).ok_or_else(|| fail("gradient shader"))?;
                if *radius > 0.0 {
                    canvas.draw_rrect(RRect::new_rect_xy(*rect, *radius, *radius), &paint);
                } else {
                    canvas.draw_rect(*rect, &paint);
                }
            }
            Op::Image { image, dst, clip } => {
                canvas.save();
                canvas.clip_rect(*clip, ClipOp::Intersect, true);
                let mut paint = SkPaint::default();
                paint.set_anti_alias(true);
                canvas.draw_image_rect_with_sampling_options(image, None, *dst, sampling, &paint);
                canvas.restore();
            }
            Op::Text { paragraph, x, y } => {
                paragraph.paint(canvas, (*x, *y));
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
    Ok(png)
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
