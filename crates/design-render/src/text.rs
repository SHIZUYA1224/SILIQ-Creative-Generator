//! Font loading and paragraph layout (TXT-002/004/005).
//!
//! Typefaces are created only from the hash-verified bytes of declared fonts
//! and registered under a private family alias in a `TypefaceFontProvider`.
//! The `FontCollection` has no default font manager and font fallback is
//! disabled. Glyph coverage is checked before shaping, and after layout every
//! run must use the requested typeface.
//!
//! The font manager used to turn bytes into a typeface depends on the Skia
//! build (docs/adr/0004): FreeType "custom empty" where available (Linux),
//! otherwise the platform manager (CoreText on macOS), which is used only for
//! `new_from_data` and is never added to the collection.

use crate::resources::{MAX_FONT_BYTES, ResourceStore};
use design_model::{Align, Code, Font, Issue, OverflowPolicy, ResolveState, TextSpec};
use design_model::{TextOutline, TextShadow};
use serde::Serialize;
use skia_safe::textlayout::{
    FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, StrutStyle, TextAlign, TextStyle, TypefaceFontProvider,
};
use skia_safe::{BlurStyle, Color, Color4f, ColorSpace, Data, FontMgr, FontStyle, MaskFilter, Paint, PaintJoin, PaintStyle, Typeface};
use std::collections::HashMap;

/// Tolerance for comparing layout extents with the text box, in du.
const FIT_EPSILON: f64 = 0.01;

pub struct LoadedFont {
    pub id: String,
    pub sha256: String,
    pub face_index: u32,
    pub family_alias: String,
    pub typeface: Typeface,
}

/// Which Skia font manager parsed the font bytes (recorded in the report,
/// because it determines the glyph rasteriser).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum FontBackend {
    /// FreeType with no system fonts (`SkFontMgr_New_Custom_Empty`).
    #[serde(rename = "freetype-custom-empty")]
    FreeTypeCustomEmpty,
    /// The platform manager (CoreText on macOS, fontconfig/FreeType on Linux),
    /// used only to parse the supplied bytes.
    #[serde(rename = "platform-from-data")]
    PlatformFromData,
}

impl FontBackend {
    /// FreeType custom-empty when this Skia build has it, else the platform.
    pub fn preferred() -> FontBackend {
        if FontMgr::custom_empty().is_some() {
            FontBackend::FreeTypeCustomEmpty
        } else {
            FontBackend::PlatformFromData
        }
    }

    fn manager(self) -> Option<FontMgr> {
        match self {
            FontBackend::FreeTypeCustomEmpty => FontMgr::custom_empty(),
            FontBackend::PlatformFromData => Some(FontMgr::new()),
        }
    }
}

pub struct FontSet {
    fonts: HashMap<String, LoadedFont>,
    collection: FontCollection,
    backend: FontBackend,
}

impl FontSet {
    /// Load the given declared fonts with the preferred backend. Every
    /// failure is returned; nothing is substituted.
    pub fn load(decls: &[&Font], store: &ResourceStore) -> Result<FontSet, Vec<Issue>> {
        Self::load_with(decls, store, FontBackend::preferred())
    }

    pub fn backend(&self) -> FontBackend {
        self.backend
    }

    pub fn load_with(decls: &[&Font], store: &ResourceStore, backend: FontBackend) -> Result<FontSet, Vec<Issue>> {
        let mgr = backend.manager().ok_or_else(|| {
            vec![Issue::error(
                Code::RenderFailed,
                format!("font backend {backend:?} is not available in this Skia build"),
                "Use a supported font backend.",
            )]
        })?;
        let mut provider = TypefaceFontProvider::new();
        let mut fonts = HashMap::new();
        let mut issues = Vec::new();
        for f in decls {
            let sha = match (f.state, &f.sha256) {
                (ResolveState::Resolved, Some(sha)) => sha,
                _ => {
                    issues.push(
                        Issue::error(
                            Code::FontMissing,
                            format!("font {:?} is unresolved (no file selected)", f.id),
                            "Select the font file for this font entry; no substitute is used.",
                        )
                        .font(&f.id),
                    );
                    continue;
                }
            };
            let bytes = match store.read("fonts", sha, &["otf", "ttf", "ttc"], MAX_FONT_BYTES) {
                Ok((b, _)) => b,
                Err(e) => {
                    issues.push(e.into_issue("font", Code::FontMissing).font(&f.id));
                    continue;
                }
            };
            let Some(typeface) = mgr.new_from_data(Data::new_copy(&bytes), Some(f.face_index)) else {
                issues.push(
                    Issue::error(
                        Code::FontMissing,
                        format!("font {:?}: file could not be parsed or face {} does not exist", f.id, f.face_index),
                        "Use a valid OpenType/TrueType file and face index.",
                    )
                    .font(&f.id),
                );
                continue;
            };
            let alias = format!("siliq-font-{}", f.id);
            provider.register_typeface(typeface.clone(), Some(alias.as_str()));
            fonts.insert(
                f.id.clone(),
                LoadedFont {
                    id: f.id.clone(),
                    sha256: sha.clone(),
                    face_index: f.face_index,
                    family_alias: alias,
                    typeface,
                },
            );
        }
        if !issues.is_empty() {
            return Err(issues);
        }
        let mut collection = FontCollection::new();
        collection.set_asset_font_manager(Some(provider.into()));
        collection.set_default_font_manager(None, None);
        collection.disable_font_fallback();
        Ok(FontSet {
            fonts,
            collection,
            backend,
        })
    }

    pub fn get(&self, id: &str) -> Option<&LoadedFont> {
        self.fonts.get(id)
    }

    pub fn loaded(&self) -> impl Iterator<Item = &LoadedFont> {
        let mut v: Vec<_> = self.fonts.values().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v.into_iter()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineInfo {
    /// UTF-8 byte range of the line in the original text.
    pub start: usize,
    pub end: usize,
    pub hard_break: bool,
    pub width: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextReport {
    pub node_id: String,
    pub font_id: String,
    pub requested_font_size: f64,
    pub applied_font_size: f64,
    pub tried_font_sizes: Vec<f64>,
    pub line_count: usize,
    pub height: f64,
    pub lines: Vec<LineInfo>,
}

pub struct LaidOutText {
    pub paragraph: Paragraph,
    pub report: TextReport,
}

/// Characters that may appear in text without a glyph of their own.
fn is_layout_control(c: char) -> bool {
    c == '\n'
}

/// Return the code points in `text` that `typeface` has no glyph for.
pub fn missing_glyphs(text: &str, typeface: &Typeface) -> Vec<char> {
    let chars: Vec<char> = text.chars().filter(|c| !is_layout_control(*c)).collect();
    let unichars: Vec<i32> = chars.iter().map(|c| *c as i32).collect();
    let mut glyphs = vec![0u16; unichars.len()];
    typeface.unichars_to_glyphs(&unichars, &mut glyphs);
    let mut missing: Vec<char> = chars.iter().zip(glyphs).filter(|(_, g)| *g == 0).map(|(c, _)| *c).collect();
    missing.dedup();
    missing
}

fn codepoints(chars: &[char]) -> String {
    let mut v: Vec<String> = chars.iter().map(|c| format!("U+{:04X}", *c as u32)).collect();
    v.sort();
    v.dedup();
    v.join(", ")
}

/// Lay out `spec` in a box of `box_w` × `box_h`. Only the author's approved
/// sizes are tried (largest first); nothing else about the text is changed.
pub fn layout_text(
    node_id: &str,
    spec: &TextSpec,
    color_rgb: [u8; 3],
    box_w: f64,
    box_h: f64,
    fonts: &FontSet,
) -> Result<LaidOutText, Issue> {
    let font = fonts.get(&spec.font_id).ok_or_else(|| {
        Issue::error(
            Code::FontMissing,
            format!("font {:?} is not loaded", spec.font_id),
            "Resolve the font.",
        )
        .node(node_id)
        .font(&spec.font_id)
    })?;

    let controls: Vec<char> = spec.value.chars().filter(|c| c.is_control() && !is_layout_control(*c)).collect();
    if !controls.is_empty() {
        return Err(Issue::error(
            Code::GlyphMissing,
            format!(
                "text contains control characters that cannot be rendered: {}",
                codepoints(&controls)
            ),
            "Remove the control characters (only U+000A line feed is allowed).",
        )
        .node(node_id)
        .font(&spec.font_id));
    }
    for (chunk, font_id, _) in styled_chunks(spec) {
        let run_font = fonts.get(font_id).ok_or_else(|| {
            Issue::error(Code::FontMissing, "declared run font is not loaded", "Resolve the font.")
                .node(node_id)
                .font(font_id)
        })?;
        let visible: String = chunk.chars().filter(|c| *c != '\u{2060}').collect();
        let missing = missing_glyphs(&visible, &run_font.typeface);
        if !missing.is_empty() {
            return Err(Issue::error(
                Code::GlyphMissing,
                format!("font {font_id:?} has no glyph for {}", codepoints(&missing)),
                "Choose a declared font that covers these characters. No fallback is used.",
            )
            .node(node_id)
            .font(font_id));
        }
    }

    let candidates: Vec<f64> = match spec.overflow_policy {
        OverflowPolicy::Error => vec![spec.font_size],
        OverflowPolicy::FitApprovedSizes => {
            // validate() guarantees font_size is in the strictly descending list.
            let start = spec.allowed_font_sizes.iter().position(|s| *s == spec.font_size).unwrap_or(0);
            spec.allowed_font_sizes[start..].to_vec()
        }
    };

    let mut tried = Vec::new();
    let mut last = None;
    for size in candidates {
        tried.push(size);
        let mut p = build_paragraph(spec, size, color_rgb, font, fonts);
        p.layout(box_w as f32);
        let mut allowed = vec![font.typeface.unique_id()];
        allowed.extend(
            spec.runs
                .iter()
                .filter_map(|r| fonts.get(&r.font_id))
                .map(|f| f.typeface.unique_id()),
        );
        verify_runs_allowed(&mut p, &allowed, node_id, &spec.font_id)?;
        let lines = p.line_number();
        let height = p.height() as f64;
        let longest = p.longest_line() as f64;
        let fits = lines <= spec.max_lines as usize && height <= box_h + FIT_EPSILON && longest <= box_w + FIT_EPSILON;
        if fits {
            let report = TextReport {
                node_id: node_id.to_owned(),
                font_id: spec.font_id.clone(),
                requested_font_size: spec.font_size,
                applied_font_size: size,
                tried_font_sizes: tried,
                line_count: lines,
                height,
                lines: line_infos(&p, spec),
            };
            return Ok(LaidOutText { paragraph: p, report });
        }
        last = Some((size, lines, height, longest));
    }
    let (size, lines, height, longest) = last.expect("at least one candidate");
    Err(Issue::error(
        Code::TextOverflow,
        format!(
            "text does not fit at any approved size (tried {:?}); at {size}: {lines} line(s) (max {}), height {height:.2} (box {box_h}), widest line {longest:.2} (box {box_w})",
            tried, spec.max_lines
        ),
        "Shorten the text, or have the author approve a different box, line count or size list. Text is never squeezed, re-tracked or truncated.",
    )
    .node(node_id)
    .font(&spec.font_id))
}

/// One extra paint pass of a text node (shadow or outline), drawn before the
/// glyph fill at an offset from the node origin.
pub struct EffectLayer {
    pub paragraph: Paragraph,
    pub dx: f32,
    pub dy: f32,
}

fn hex_rgb(c: &str) -> [u8; 3] {
    let v = |i: usize| u8::from_str_radix(&c[i..i + 2], 16).expect("validated color");
    [v(1), v(3), v(5)]
}

fn effect_paint(rgb: [u8; 3], alpha: f64, outline_width: Option<f64>) -> Paint {
    let mut p = Paint::new(
        Color4f::new(rgb[0] as f32 / 255.0, rgb[1] as f32 / 255.0, rgb[2] as f32 / 255.0, alpha as f32),
        &ColorSpace::new_srgb(),
    );
    p.set_anti_alias(true);
    match outline_width {
        // The stroke is centred on the glyph edge; twice the visible width
        // puts `width` du outside the edge (the inside half is under the fill).
        Some(w) => {
            p.set_style(PaintStyle::StrokeAndFill)
                .set_stroke_width((2.0 * w) as f32)
                .set_stroke_join(PaintJoin::Round);
        }
        None => {
            p.set_style(PaintStyle::Fill);
        }
    }
    p
}

/// Build the shadow and outline passes for a laid-out text node, in paint
/// order. Each pass is laid out independently with the same style, size and
/// width; its line breaks must match the fill pass exactly or rendering stops.
pub fn effect_layers(
    node_id: &str,
    spec: &TextSpec,
    laid: &LaidOutText,
    box_w: f64,
    outline: Option<&TextOutline>,
    shadow: Option<&TextShadow>,
    fonts: &FontSet,
) -> Result<Vec<EffectLayer>, Issue> {
    let font = fonts.get(&spec.font_id).expect("layout_text succeeded");
    let size = laid.report.applied_font_size;
    let outline_w = outline.map(|o| o.width);
    let mut passes: Vec<(Paint, f32, f32)> = Vec::new();
    if let Some(sh) = shadow {
        let mut p = effect_paint(hex_rgb(&sh.color), sh.opacity, outline_w);
        if sh.blur > 0.0 {
            // Skia's radius → sigma convention.
            let sigma = (0.577_35 * sh.blur + 0.5) as f32;
            p.set_mask_filter(MaskFilter::blur(BlurStyle::Normal, sigma, false));
        }
        passes.push((p, sh.offset_x as f32, sh.offset_y as f32));
    }
    if let Some(o) = outline {
        passes.push((effect_paint(hex_rgb(&o.color), 1.0, Some(o.width)), 0.0, 0.0));
    }
    let reference = line_signature(&laid.paragraph);
    let mut layers = Vec::new();
    for (paint, dx, dy) in passes {
        let mut p = build_paragraph_with(spec, size, Foreground::Paint(&paint), font, fonts);
        p.layout(box_w as f32);
        if line_signature(&p) != reference {
            return Err(Issue::error(
                Code::RenderFailed,
                "an outline/shadow pass did not lay out identically to the text",
                "Report this as an engine defect.",
            )
            .node(node_id));
        }
        layers.push(EffectLayer { paragraph: p, dx, dy });
    }
    Ok(layers)
}

fn line_signature(p: &Paragraph) -> Vec<(usize, usize, u64)> {
    p.get_line_metrics()
        .iter()
        .map(|m| (m.start_index, m.end_index, m.width.to_bits()))
        .collect()
}

/// How the glyphs of one paragraph pass are painted.
#[derive(Clone, Copy)]
enum Foreground<'a> {
    Color([u8; 3]),
    Paint(&'a Paint),
}

fn build_paragraph(spec: &TextSpec, size: f64, rgb: [u8; 3], font: &LoadedFont, fonts: &FontSet) -> Paragraph {
    build_paragraph_with(spec, size, Foreground::Color(rgb), font, fonts)
}

fn build_paragraph_with(spec: &TextSpec, size: f64, fg: Foreground, font: &LoadedFont, fonts: &FontSet) -> Paragraph {
    let families = [font.family_alias.as_str()];
    // lineHeight is an absolute distance in du; it is kept as authored even
    // when an approved smaller font size is applied.
    let height_multiplier = (spec.line_height / size) as f32;

    let mut ts = TextStyle::new();
    ts.set_font_families(&families)
        .set_font_size(size as f32)
        .set_font_style(FontStyle::normal())
        .set_letter_spacing(spec.letter_spacing as f32)
        .set_height(height_multiplier)
        .set_height_override(true)
        .set_half_leading(true)
        .set_locale(spec.language.bcp47());
    for feature in &spec.features {
        ts.add_font_feature(&feature.tag, feature.value as i32);
    }
    match fg {
        Foreground::Color(rgb) => {
            ts.set_color(Color::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
        Foreground::Paint(p) => {
            ts.set_foreground_paint(p);
        }
    }

    // A forced strut gives every line exactly `lineHeight`, independent of
    // which glyphs the line contains.
    let mut strut = StrutStyle::new();
    strut
        .set_font_families(&families)
        .set_font_style(FontStyle::normal())
        .set_font_size(size as f32)
        .set_height(height_multiplier)
        .set_height_override(true)
        .set_half_leading(true)
        .set_strut_enabled(true)
        .set_force_strut_height(true);

    let mut ps = ParagraphStyle::new();
    ps.set_text_style(&ts)
        .set_strut_style(strut)
        .set_text_align(match spec.align {
            Align::Left => TextAlign::Left,
            Align::Center => TextAlign::Center,
            Align::Right => TextAlign::Right,
        })
        .set_apply_rounding_hack(false)
        .set_replace_tab_characters(false);
    ps.turn_hinting_off();

    let mut b = ParagraphBuilder::new(&ps, fonts.collection.clone());
    b.push_style(&ts);
    for (chunk, font_id, color) in styled_chunks(spec) {
        let mut run_style = ts.clone();
        if let Some(font) = fonts.get(font_id) {
            run_style.set_font_families(&[font.family_alias.as_str()]);
        }
        if matches!(fg, Foreground::Color(_))
            && let Some(color) = color
        {
            let rgb = hex_rgb(color);
            run_style.set_color(Color::from_rgb(rgb[0], rgb[1], rgb[2]));
        }
        b.push_style(&run_style);
        b.add_text(&chunk);
        b.pop();
    }
    b.build()
}

/// After layout, refuse any run that is not the requested typeface or any
/// glyph Skia could not resolve — a second guard against silent fallback.
#[cfg(test)]
fn verify_runs(p: &mut Paragraph, font: &LoadedFont, node_id: &str, font_id: &str) -> Result<(), Issue> {
    verify_runs_allowed(p, &[font.typeface.unique_id()], node_id, font_id)
}
fn verify_runs_allowed(p: &mut Paragraph, allowed: &[u32], node_id: &str, font_id: &str) -> Result<(), Issue> {
    if p.unresolved_glyphs().unwrap_or(0) > 0 {
        return Err(Issue::error(
            Code::GlyphMissing,
            "layout reported unresolved glyphs",
            "Use a font that covers the text.",
        )
        .node(node_id)
        .font(font_id));
    }
    let mut foreign = false;
    p.visit(|_, info| {
        if let Some(info) = info
            && !allowed.contains(&info.font().typeface().unique_id())
        {
            foreign = true;
        }
    });
    if foreign {
        return Err(Issue::error(
            Code::GlyphMissing,
            "layout used a typeface other than the requested font",
            "This indicates a font fallback; the text cannot be rendered with the requested font.",
        )
        .node(node_id)
        .font(font_id));
    }
    Ok(())
}

fn joins(spec: &TextSpec, c: char, next: char) -> bool {
    spec.kinsoku == design_model::KinsokuPolicy::StrictJapanese
        && spec.language == design_model::Language::Ja
        && c != '\n'
        && next != '\n'
        && ("、。，．・：；？！ー々ぁぃぅぇぉっゃゅょゎァィゥェォッャュョヮヵヶ）〕］｝〉》」』】〙〛’”".contains(next)
            || "（〔［｛〈《「『【〘〚‘“".contains(c))
}

fn styled_chunks(spec: &TextSpec) -> Vec<(String, &str, Option<&str>)> {
    let chars: Vec<_> = spec.value.char_indices().collect();
    let mut chunks: Vec<(String, &str, Option<&str>)> = Vec::new();
    for (i, (byte, c)) in chars.iter().enumerate() {
        let run = spec.runs.iter().find(|r| r.start <= *byte && *byte < r.end);
        let font_id = run.map(|r| r.font_id.as_str()).unwrap_or(&spec.font_id);
        let color = run.and_then(|r| r.color.as_deref());
        if chunks.last().is_none_or(|(_, f, col)| *f != font_id || *col != color) {
            chunks.push((String::new(), font_id, color));
        }
        let chunk = &mut chunks.last_mut().unwrap().0;
        chunk.push(*c);
        if let Some((_, next)) = chars.get(i + 1)
            && joins(spec, *c, *next)
        {
            chunk.push('\u{2060}');
        }
    }
    if chunks.is_empty() {
        chunks.push((String::new(), &spec.font_id, None));
    }
    chunks
}

/// Strict-Japanese word-joiner boundaries are derived layout data only. The
/// authored string stays byte-for-byte unchanged and report offsets refer to it.
fn layout_string(spec: &TextSpec) -> (String, Vec<(usize, usize)>) {
    let mut result = String::new();
    let mut map = vec![(0, 0)];
    let mut units = 0;
    let chars: Vec<(usize, char)> = spec.value.char_indices().collect();
    for (i, (byte, c)) in chars.iter().enumerate() {
        result.push(*c);
        units += c.len_utf16();
        let next_byte = *byte + c.len_utf8();
        map.push((units, next_byte));
        if let Some((_, next)) = chars.get(i + 1)
            && joins(spec, *c, *next)
        {
            result.push('\u{2060}');
            units += 1;
            map.push((units, next_byte));
        }
    }
    (result, map)
}

fn line_infos(p: &Paragraph, spec: &TextSpec) -> Vec<LineInfo> {
    let text = &spec.value;
    let (_, mapping) = layout_string(spec);
    // SkParagraph reports line indices in UTF-16 code units (verified by the
    // astral-plane test in tests/render.rs); convert them to UTF-8 offsets.
    let to_utf8 = |u16_index: usize| -> usize {
        mapping
            .iter()
            .rev()
            .find(|(units, _)| *units <= u16_index)
            .map(|(_, byte)| *byte)
            .unwrap_or(0)
            .min(text.len())
    };
    p.get_line_metrics()
        .iter()
        .map(|m| LineInfo {
            start: to_utf8(m.start_index),
            end: to_utf8(m.end_excluding_whitespaces),
            hard_break: m.hard_break,
            width: m.width,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use design_model::{PackagePolicy, ResolveState};
    use std::path::Path;

    /// The post-layout guard must catch unresolved glyphs on its own, i.e.
    /// even if the pre-shaping coverage check were bypassed.
    #[test]
    fn post_layout_guard_detects_unresolved_glyphs() {
        let store = ResourceStore::open(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/public-safe/resources"
        )))
        .unwrap();
        let decl = Font {
            id: "f".into(),
            name_hint: String::new(),
            state: ResolveState::Resolved,
            sha256: Some("dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073".into()),
            face_index: 0,
            package_policy: PackagePolicy::EmbedConfirmed,
        };
        for backend in [FontBackend::FreeTypeCustomEmpty, FontBackend::PlatformFromData] {
            if backend.manager().is_none() {
                continue;
            }
            guard_checks(&decl, &store, backend);
        }
    }

    fn guard_checks(decl: &Font, store: &ResourceStore, backend: FontBackend) {
        let fonts = FontSet::load_with(&[decl], store, backend).unwrap();
        let font = fonts.get("f").unwrap();
        let spec = TextSpec {
            value: "日本語ภาษาไทย".into(),
            font_id: "f".into(),
            font_size: 40.0,
            line_height: 50.0,
            letter_spacing: 0.0,
            max_lines: 2,
            overflow_policy: OverflowPolicy::Error,
            allowed_font_sizes: vec![],
            language: design_model::Language::Ja,
            align: Align::Left,
            features: vec![],
            kinsoku: design_model::KinsokuPolicy::Unicode,
            runs: vec![],
        };
        let mut p = build_paragraph(&spec, 40.0, [0, 0, 0], font, &fonts);
        p.layout(1000.0);
        let err = verify_runs(&mut p, font, "n", "f").unwrap_err();
        assert_eq!(err.code, Code::GlyphMissing);

        let ok = TextSpec {
            value: "日本語".into(),
            ..spec
        };
        let mut p = build_paragraph(&ok, 40.0, [0, 0, 0], font, &fonts);
        p.layout(1000.0);
        assert!(verify_runs(&mut p, font, "n", "f").is_ok());
    }
}
