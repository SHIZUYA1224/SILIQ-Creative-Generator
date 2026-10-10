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
use serde::Serialize;
use skia_safe::textlayout::{
    FontCollection, Paragraph, ParagraphBuilder, ParagraphStyle, StrutStyle, TextAlign, TextStyle, TypefaceFontProvider,
};
use skia_safe::{Color, Data, FontMgr, FontStyle, Typeface};
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
    let missing = missing_glyphs(&spec.value, &font.typeface);
    if !missing.is_empty() {
        return Err(Issue::error(
            Code::GlyphMissing,
            format!("font {:?} has no glyph for {}", spec.font_id, codepoints(&missing)),
            "Change the text, or have the author choose a font that covers these characters. No fallback font is used.",
        )
        .node(node_id)
        .font(&spec.font_id));
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
        verify_runs(&mut p, font, node_id, &spec.font_id)?;
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
                lines: line_infos(&p, &spec.value),
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

fn build_paragraph(spec: &TextSpec, size: f64, rgb: [u8; 3], font: &LoadedFont, fonts: &FontSet) -> Paragraph {
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
        .set_locale(spec.language.bcp47())
        .set_color(Color::from_rgb(rgb[0], rgb[1], rgb[2]));

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
    b.add_text(&spec.value);
    b.build()
}

/// After layout, refuse any run that is not the requested typeface or any
/// glyph Skia could not resolve — a second guard against silent fallback.
fn verify_runs(p: &mut Paragraph, font: &LoadedFont, node_id: &str, font_id: &str) -> Result<(), Issue> {
    if p.unresolved_glyphs().unwrap_or(0) > 0 {
        return Err(Issue::error(
            Code::GlyphMissing,
            "layout reported unresolved glyphs",
            "Use a font that covers the text.",
        )
        .node(node_id)
        .font(font_id));
    }
    let want = font.typeface.unique_id();
    let mut foreign = false;
    p.visit(|_, info| {
        if let Some(info) = info
            && info.font().typeface().unique_id() != want
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

fn line_infos(p: &Paragraph, text: &str) -> Vec<LineInfo> {
    // SkParagraph reports line indices in UTF-16 code units (verified by the
    // astral-plane test in tests/render.rs); convert them to UTF-8 offsets.
    let to_utf8 = |u16_index: usize| -> usize {
        let mut units = 0;
        for (byte, c) in text.char_indices() {
            if units >= u16_index {
                return byte;
            }
            units += c.len_utf16();
        }
        text.len()
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
