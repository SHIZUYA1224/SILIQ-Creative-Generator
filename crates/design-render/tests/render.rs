//! End-to-end tests of the Phase 0 render path against public-safe fixtures.

use design_model::Code;
use design_render::resources::{ResourceStore, sha256_hex};
use design_render::text::FontBackend;
use design_render::text::missing_glyphs;
use design_render::{RenderOptions, RenderOutput, render, render_with};
use serde_json::{Value, json};
use skia_safe::{AlphaType, ColorType, FontMgr, Image, ImageInfo};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/public-safe");
const FONT_SHA: &str = "dff723ba59d57d136764a04b9b2d03205544f7cd785a711442d6d2d085ac5073";
const PHOTO_SHA: &str = "f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3d9a1995bde5cda0";
const LOGO_SHA: &str = "08317af1678409f3a1dcf29b651f12b439d311bcddefdffd3580cd4b0067c96c";

fn fixture_doc() -> Value {
    serde_json::from_str(&fs::read_to_string(format!("{FIXTURES}/documents/basic.json")).unwrap()).unwrap()
}

fn store() -> ResourceStore {
    ResourceStore::open(Path::new(&format!("{FIXTURES}/resources"))).unwrap()
}

fn node_mut<'a>(v: &'a mut Value, id: &str) -> &'a mut Value {
    v["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["id"] == id).unwrap()
}

fn run(doc: &Value, variant: &str, store: &ResourceStore) -> Result<RenderOutput, Vec<design_model::Issue>> {
    render(doc.to_string().as_bytes(), variant, store)
}

fn codes(doc: &Value, variant: &str, store: &ResourceStore) -> Vec<Code> {
    match run(doc, variant, store) {
        Ok(_) => vec![],
        Err(issues) => issues.into_iter().map(|i| i.code).collect(),
    }
}

/// A private copy of the fixture resource store that a test may modify.
fn temp_store() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("store-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&dir);
    for sub in ["assets", "fonts"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
        for e in fs::read_dir(format!("{FIXTURES}/resources/{sub}")).unwrap() {
            let e = e.unwrap();
            fs::copy(e.path(), dir.join(sub).join(e.file_name())).unwrap();
        }
    }
    dir
}

/// Add raw bytes to a store under their real hash; returns the hash.
fn add_asset(dir: &Path, bytes: &[u8], ext: &str) -> String {
    let sha = sha256_hex(bytes);
    fs::write(dir.join("assets").join(format!("{sha}.{ext}")), bytes).unwrap();
    sha
}

fn pixels(png: &[u8]) -> (i32, i32, Vec<u8>) {
    let img = Image::from_encoded(skia_safe::Data::new_copy(png)).unwrap();
    let (w, h) = (img.width(), img.height());
    let info = ImageInfo::new((w, h), ColorType::RGBA8888, AlphaType::Unpremul, None);
    let mut buf = vec![0u8; (w * h * 4) as usize];
    assert!(img.read_pixels(&info, &mut buf, (w * 4) as usize, (0, 0), skia_safe::image::CachingHint::Disallow));
    (w, h, buf)
}

fn px(p: &(i32, i32, Vec<u8>), x: i32, y: i32) -> [u8; 4] {
    let i = ((y * p.0 + x) * 4) as usize;
    [p.2[i], p.2[i + 1], p.2[i + 2], p.2[i + 3]]
}

fn close(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
}

// ------------------------------------------------------------ normal --

#[test]
fn renders_square_fixture() {
    let out = run(&fixture_doc(), "square", &store()).unwrap();
    let p = pixels(&out.png);
    assert_eq!((p.0, p.1), (1080, 1080));
    assert_eq!(out.report.output_sha256, sha256_hex(&out.png));
    assert!(out.report.warnings.is_empty());

    // Cover + focal point: the disc (image 1040,400) lands at (684,300).
    assert!(close(px(&p, 684, 300), [245, 200, 60, 255], 2), "{:?}", px(&p, 684, 300));
    // Clip: the cover-scaled photo is 600 tall but its box ends at y=620.
    let below = px(&p, 300, 640);
    assert!(
        below[0].abs_diff(below[2]) < 16 && below[3] == 255,
        "expected grey background, got {below:?}"
    );
    // Background gradient: 90° runs top → bottom (#F4F4F4 → #D0D4DC).
    assert!(close(px(&p, 5, 1), [0xF4, 0xF4, 0xF4, 255], 2), "{:?}", px(&p, 5, 1));
    assert!(close(px(&p, 5, 1078), [0xD0, 0xD4, 0xDC, 255], 2), "{:?}", px(&p, 5, 1078));
    // Transparent photo edge blends with the background (alpha fade).
    let edge = px(&p, 61, 300);
    assert!(edge[3] == 255);

    let head = out.report.text.iter().find(|t| t.node_id == "headline").unwrap();
    assert_eq!(head.applied_font_size, 64.0);
    assert_eq!(head.line_count, 2);
}

#[test]
fn renders_portrait_variant_with_patched_font_size() {
    let out = run(&fixture_doc(), "portrait", &store()).unwrap();
    let p = pixels(&out.png);
    assert_eq!((p.0, p.1), (1080, 1350));
    let head = out.report.text.iter().find(|t| t.node_id == "headline").unwrap();
    assert_eq!(head.requested_font_size, 60.0);
    assert_eq!(head.applied_font_size, 60.0);
}

#[test]
fn repeated_renders_are_byte_identical() {
    let doc = fixture_doc();
    let s = store();
    let hashes: Vec<String> = (0..10).map(|_| sha256_hex(&run(&doc, "square", &s).unwrap().png)).collect();
    assert!(hashes.iter().all(|h| h == &hashes[0]), "{hashes:?}");
}

#[test]
fn png_has_no_text_or_time_chunks() {
    let png = run(&fixture_doc(), "square", &store()).unwrap().png;
    let mut i = 8;
    let mut chunks = Vec::new();
    while i + 8 <= png.len() {
        let len = u32::from_be_bytes(png[i..i + 4].try_into().unwrap()) as usize;
        chunks.push(String::from_utf8_lossy(&png[i + 4..i + 8]).into_owned());
        i += 12 + len;
    }
    for forbidden in ["tEXt", "zTXt", "iTXt", "tIME", "eXIf"] {
        assert!(!chunks.iter().any(|c| c == forbidden), "{chunks:?}");
    }
    // Colour information is present (sRGB tag or ICC profile).
    assert!(chunks.iter().any(|c| c == "sRGB" || c == "iCCP"), "{chunks:?}");
}

#[test]
fn fit_uses_only_approved_sizes_in_order() {
    let mut doc = fixture_doc();
    // 32 full-width kana: 3 lines at 61–64 (15 per line), 2 lines at 60 (16 per line).
    node_mut(&mut doc, "headline")["text"]["value"] = json!("あいうえおかきくけこさしすせそたちつてとなにぬねのはひふへほまみ");
    let out = run(&doc, "square", &store()).unwrap();
    let head = out.report.text.iter().find(|t| t.node_id == "headline").unwrap();
    assert_eq!(head.tried_font_sizes, vec![64.0, 63.0, 62.0, 61.0, 60.0]);
    assert_eq!(head.applied_font_size, 60.0);
    assert_eq!(head.line_count, 2);
}

#[test]
fn line_ranges_are_utf8_offsets_even_for_astral_characters() {
    let mut doc = fixture_doc();
    let text = "𠮷野家の𠮷";
    node_mut(&mut doc, "caption")["text"]["value"] = json!(text);
    let out = run(&doc, "square", &store()).unwrap();
    let cap = out.report.text.iter().find(|t| t.node_id == "caption").unwrap();
    assert_eq!(cap.lines.len(), 1);
    assert_eq!((cap.lines[0].start, cap.lines[0].end), (0, text.len()));
}

/// Records how SkParagraph/ICU breaks Japanese lines with this build. Rows
/// marked GAP differ from JLREQ strict line-start rules (small kana and the
/// prolonged sound mark may start a line); kinsoku beyond ICU is Phase 1.
/// A change in any row means the text stack changed and must be re-reviewed.
#[test]
fn japanese_line_break_behaviour_table() {
    let cases: &[(&str, &[&str])] = &[
        ("あいうえお。かきくけこ", &["あいうえ", "お。かきく", "けこ"]),
        ("あいうえお、かきくけこ", &["あいうえ", "お、かきく", "けこ"]),
        ("あいうえお」かきくけこ", &["あいうえ", "お」かきく", "けこ"]),
        ("あいうえお）かきくけこ", &["あいうえ", "お）かきく", "けこ"]),
        ("あいうえお！かきくけこ", &["あいうえ", "お！かきく", "けこ"]),
        ("あいうえお？かきくけこ", &["あいうえ", "お？かきく", "けこ"]),
        ("あいうえお・かきくけこ", &["あいうえ", "お・かきく", "けこ"]),
        ("あいうえお…かきくけこ", &["あいうえ", "お…かきく", "けこ"]),
        ("あいうえお々かきくけこ", &["あいうえ", "お々かきく", "けこ"]),
        ("あいうえ「おかきくけこ", &["あいうえ", "「おかきく", "けこ"]),
        ("あいうえ（おかきくけこ", &["あいうえ", "（おかきく", "けこ"]),
        // GAP: JLREQ strict forbids these at line start.
        ("あいうえおーかきくけこ", &["あいうえお", "ーかきくけ", "こ"]),
        ("あいうえおゃかきくけこ", &["あいうえお", "ゃかきくけ", "こ"]),
        ("あいうえおっかきくけこ", &["あいうえお", "っかきくけ", "こ"]),
        ("あいうえおァかきくけこ", &["あいうえお", "ァかきくけ", "こ"]),
        // Numbers and Latin words are not split.
        ("価格は1,980円です。", &["価格は", "1,980円で", "す。"]),
    ];
    let s = store();
    for (text, expected) in cases {
        let mut doc = fixture_doc();
        let cap = node_mut(&mut doc, "caption");
        cap["bounds"]["width"] = json!(200);
        cap["bounds"]["height"] = json!(200);
        let t = &mut cap["text"];
        t["value"] = json!(text);
        t["fontSize"] = json!(40);
        t["lineHeight"] = json!(50);
        t["letterSpacing"] = json!(0);
        t["maxLines"] = json!(4);
        t["align"] = json!("left");
        let out = run(&doc, "square", &s).unwrap();
        let rep = out.report.text.iter().find(|t| t.node_id == "caption").unwrap();
        let got: Vec<&str> = rep.lines.iter().map(|l| &text[l.start..l.end]).collect();
        assert_eq!(&got, expected, "line breaks for {text}");
    }
}

#[test]
fn upscaled_image_produces_a_warning_not_silence() {
    let mut doc = fixture_doc();
    node_mut(&mut doc, "photo")["bounds"] = json!({"x": 0, "y": 0, "width": 1080, "height": 1350});
    let out = run(&doc, "square", &store()).unwrap();
    assert!(out.report.warnings.iter().any(|w| w.code == Code::ImageUpscaled));
}

// ------------------------------------------------- variants & sample --

#[test]
fn product_sample_cannot_be_rendered() {
    let sample: Value = serde_json::from_str(include_str!("../../../examples/design.core.sample.json")).unwrap();
    assert_eq!(codes(&sample, "square", &store()), vec![Code::VariantNotApproved]);

    // Even with the variant approved, unresolved fonts/assets block the render.
    let mut approved = sample.clone();
    approved["variants"][0]["approvalStatus"] = json!("approved");
    let c = codes(&approved, "square", &store());
    assert!(c.contains(&Code::FontMissing), "{c:?}");
    assert_eq!(c.iter().filter(|c| **c == Code::AssetMissing).count(), 2, "{c:?}");
}

#[test]
fn draft_and_unknown_variants_are_refused() {
    assert_eq!(codes(&fixture_doc(), "wide-draft", &store()), vec![Code::VariantNotApproved]);
    assert_eq!(codes(&fixture_doc(), "landscape", &store()), vec![Code::LayoutUndefined]);
}

#[test]
fn fractional_variant_size_is_refused() {
    let mut doc = fixture_doc();
    doc["variants"][0]["width"] = json!(1080.5);
    assert_eq!(codes(&doc, "square", &store()), vec![Code::ExportUnsupported]);
}

// ------------------------------------------------------------- fonts --

#[test]
fn unresolved_font_is_missing() {
    let mut doc = fixture_doc();
    doc["fonts"][0]["state"] = json!("unresolved");
    doc["fonts"][0]["sha256"] = Value::Null;
    assert_eq!(codes(&doc, "square", &store()), vec![Code::FontMissing]);
}

#[test]
fn absent_font_file_is_missing_and_not_substituted() {
    let mut doc = fixture_doc();
    doc["fonts"][0]["sha256"] = json!("0".repeat(64));
    let err = run(&doc, "square", &store()).err().unwrap();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code, Code::FontMissing);
    assert_eq!(err[0].location.font_id.as_deref(), Some("noto-sans-jp"));
}

#[test]
fn font_bytes_must_match_hash() {
    let dir = temp_store();
    // Same name, different bytes.
    let f = dir.join("fonts").join(format!("{FONT_SHA}.otf"));
    let mut bytes = fs::read(&f).unwrap();
    bytes[1000] ^= 0xFF;
    fs::write(&f, bytes).unwrap();
    let s = ResourceStore::open(&dir).unwrap();
    assert_eq!(codes(&fixture_doc(), "square", &s), vec![Code::AssetChecksum]);
}

#[test]
fn invalid_face_index_is_reported() {
    let mut doc = fixture_doc();
    doc["fonts"][0]["faceIndex"] = json!(3);
    assert_eq!(codes(&doc, "square", &store()), vec![Code::FontMissing]);
}

#[test]
fn missing_glyphs_detected_by_codepoint() {
    let mgr = FontMgr::custom_empty().unwrap();
    let bytes = fs::read(format!("{FIXTURES}/resources/fonts/{FONT_SHA}.otf")).unwrap();
    let tf = mgr.new_from_data(skia_safe::Data::new_copy(&bytes), 0).unwrap();
    assert!(missing_glyphs("品質を、崩さず量産する。ABC abc 123 ー々〆", &tf).is_empty());
    assert_eq!(missing_glyphs("日本語ภาษา", &tf), vec!['ภ', 'า', 'ษ', 'า']);
    assert_eq!(missing_glyphs("😀", &tf), vec!['😀']);
}

#[test]
fn glyph_missing_stops_render_without_fallback() {
    for text in ["タイ語ภาษาไทย", "絵文字😀", "Hangul 한국어"] {
        let mut doc = fixture_doc();
        node_mut(&mut doc, "caption")["text"]["value"] = json!(text);
        let err = run(&doc, "square", &store()).err().unwrap_or_else(|| panic!("{text} rendered"));
        assert_eq!(err.len(), 1, "{err:?}");
        assert_eq!(err[0].code, Code::GlyphMissing);
        assert_eq!(err[0].location.node_id.as_deref(), Some("caption"));
        assert!(err[0].message.contains("U+"), "{}", err[0].message);
    }
}

#[test]
fn control_characters_are_refused() {
    let mut doc = fixture_doc();
    node_mut(&mut doc, "caption")["text"]["value"] = json!("tab\there");
    assert_eq!(codes(&doc, "square", &store()), vec![Code::GlyphMissing]);
}

// ---------------------------------------------------------- overflow --

#[test]
fn overflow_with_error_policy_stops() {
    let mut doc = fixture_doc();
    node_mut(&mut doc, "caption")["text"]["value"] =
        json!("この一行はキャプションの枠に収まらないほど長い文章であり、縮小は許可されていません。");
    let err = run(&doc, "square", &store()).err().unwrap();
    assert_eq!(err.len(), 1);
    assert_eq!(err[0].code, Code::TextOverflow);
    assert_eq!(err[0].location.node_id.as_deref(), Some("caption"));
}

#[test]
fn overflow_after_all_approved_sizes_stops() {
    let mut doc = fixture_doc();
    node_mut(&mut doc, "headline")["text"]["value"] = json!("あ".repeat(40));
    let err = run(&doc, "square", &store()).err().unwrap();
    assert_eq!(err[0].code, Code::TextOverflow);
    assert!(
        err[0].message.contains("[64.0, 63.0, 62.0, 61.0, 60.0, 59.0, 58.0, 57.0, 56.0]"),
        "{}",
        err[0].message
    );
}

#[test]
fn manual_line_breaks_count_against_max_lines() {
    let mut doc = fixture_doc();
    node_mut(&mut doc, "headline")["text"]["value"] = json!("一行目\n二行目\n三行目");
    assert_eq!(codes(&doc, "square", &store()), vec![Code::TextOverflow]);
}

// ------------------------------------------------------------ assets --

#[test]
fn unresolved_and_absent_assets_are_missing() {
    let mut doc = fixture_doc();
    doc["assets"][0]["state"] = json!("unresolved");
    doc["assets"][0]["sha256"] = Value::Null;
    doc["assets"][1]["sha256"] = json!("1".repeat(64));
    let c = codes(&doc, "square", &store());
    assert_eq!(c, vec![Code::AssetMissing, Code::AssetMissing]);
}

#[test]
fn declared_type_must_match_content() {
    let dir = temp_store();
    let photo = fs::read(dir.join("assets").join(format!("{PHOTO_SHA}.png"))).unwrap();
    let sha = add_asset(&dir, &photo, "jpg");
    let mut doc = fixture_doc();
    doc["assets"][0]["sha256"] = json!(sha);
    doc["assets"][0]["mimeHint"] = json!("image/jpeg");
    assert_eq!(codes(&doc, "square", &ResourceStore::open(&dir).unwrap()), vec![Code::AssetType]);
}

#[test]
fn corrupt_image_is_refused() {
    let dir = temp_store();
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(&[0x13; 64]);
    let sha = add_asset(&dir, &bytes, "png");
    let mut doc = fixture_doc();
    doc["assets"][0]["sha256"] = json!(sha);
    assert_eq!(codes(&doc, "square", &ResourceStore::open(&dir).unwrap()), vec![Code::AssetType]);
}

#[test]
fn webp_is_refused_in_this_build() {
    let dir = temp_store();
    let sha = add_asset(&dir, b"RIFF\x10\0\0\0WEBPVP8 \x04\0\0\0\0\0\0\0", "webp");
    let mut doc = fixture_doc();
    doc["assets"][0]["sha256"] = json!(sha);
    doc["assets"][0]["mimeHint"] = json!("image/webp");
    assert_eq!(codes(&doc, "square", &ResourceStore::open(&dir).unwrap()), vec![Code::AssetType]);
}

#[test]
fn oversized_image_dimensions_are_refused_before_decoding() {
    let dir = temp_store();
    let mut surface = skia_safe::surfaces::raster_n32_premul((16_385, 1)).unwrap();
    let img = surface.image_snapshot();
    let png = img.encode(None, skia_safe::EncodedImageFormat::PNG, None).unwrap();
    let sha = add_asset(&dir, png.as_bytes(), "png");
    let mut doc = fixture_doc();
    doc["assets"][0]["sha256"] = json!(sha);
    assert_eq!(
        codes(&doc, "square", &ResourceStore::open(&dir).unwrap()),
        vec![Code::ResourceLimit]
    );
}

#[test]
fn unsafe_svg_logo_stops_render() {
    let cases: &[(&str, Code)] = &[
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><script>x()</script></svg>"#,
            Code::SvgUnsupported,
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><image xlink:href="https://example.com/x.png" width="10" height="10"/></svg>"#,
            Code::ExternalReference,
        ),
        (
            r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><text>LOGO</text></svg>"#,
            Code::SvgUnsupported,
        ),
        (
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle r="4" cx="5" cy="5" stroke="#000"/></svg>"##,
            Code::SvgUnsupported,
        ),
    ];
    for (src, code) in cases {
        let dir = temp_store();
        let sha = add_asset(&dir, src.as_bytes(), "svg");
        let mut doc = fixture_doc();
        doc["assets"][1]["sha256"] = json!(sha);
        let err = run(&doc, "square", &ResourceStore::open(&dir).unwrap()).err().unwrap();
        assert_eq!(err.len(), 1, "{err:?}");
        assert_eq!(err[0].code, *code, "{src}");
        assert_eq!(err[0].location.node_id.as_deref(), Some("brand-logo"));
    }
}

#[test]
fn logo_below_min_width_is_refused() {
    let mut doc = fixture_doc();
    node_mut(&mut doc, "brand-logo")["minWidth"] = json!(300);
    assert_eq!(codes(&doc, "square", &store()), vec![Code::LogoMinWidth]);
}

#[test]
fn all_independent_problems_are_reported_together() {
    let mut doc = fixture_doc();
    doc["fonts"][0]["sha256"] = json!("0".repeat(64));
    doc["assets"][1]["sha256"] = json!("1".repeat(64));
    let c = codes(&doc, "square", &store());
    assert!(c.contains(&Code::FontMissing) && c.contains(&Code::AssetMissing), "{c:?}");
}

// ---------------------------------------------------- resource store --

#[cfg(unix)]
#[test]
fn symlinked_resources_are_refused() {
    let dir = temp_store();
    let font = dir.join("fonts").join(format!("{FONT_SHA}.otf"));
    let real = dir.join("real-font.otf");
    fs::rename(&font, &real).unwrap();
    std::os::unix::fs::symlink(&real, &font).unwrap();
    assert_eq!(
        codes(&fixture_doc(), "square", &ResourceStore::open(&dir).unwrap()),
        vec![Code::PathDenied]
    );

    // A symlinked assets/ directory is refused as well.
    let dir = temp_store();
    let assets = dir.join("assets");
    let moved = dir.join("elsewhere");
    fs::rename(&assets, &moved).unwrap();
    std::os::unix::fs::symlink(&moved, &assets).unwrap();
    let c = codes(&fixture_doc(), "square", &ResourceStore::open(&dir).unwrap());
    assert!(!c.is_empty() && c.iter().all(|c| *c == Code::PathDenied), "{c:?}");

    // And a symlink as the store root.
    let link = dir.with_extension("link");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&dir, &link).unwrap();
    assert_eq!(ResourceStore::open(&link).unwrap_err().code, Code::PathDenied);
}

#[test]
fn store_only_reads_hash_named_files() {
    let s = store();
    assert!(s.read("assets", "../../../etc/passwd", &["png"], 1024).is_err());
    assert!(s.read("assets", &LOGO_SHA.to_uppercase(), &["svg"], 1 << 20).is_err());
    assert!(s.read("assets", LOGO_SHA, &["svg"], 1 << 20).is_ok());
}

// ------------------------------------------------- font backend paths --

/// The platform font manager is what a macOS build uses (no FreeType in the
/// official macOS Skia binary). On Linux it is fontconfig/FreeType; this test
/// checks that the guards and determinism hold on that code path, not that
/// macOS renders identically.
#[test]
fn platform_font_backend_keeps_guards_and_determinism() {
    let opts = RenderOptions {
        font_backend: FontBackend::PlatformFromData,
    };
    let doc = fixture_doc();
    let s = store();
    let a = render_with(doc.to_string().as_bytes(), "square", &s, opts).unwrap();
    let b = render_with(doc.to_string().as_bytes(), "square", &s, opts).unwrap();
    assert_eq!(a.png, b.png);
    assert_eq!(a.report.font_backend, Some(FontBackend::PlatformFromData));

    let mut bad = fixture_doc();
    node_mut(&mut bad, "caption")["text"]["value"] = json!("タイ語ภาษาไทย");
    let err = render_with(bad.to_string().as_bytes(), "square", &s, opts).err().unwrap();
    assert_eq!(err[0].code, Code::GlyphMissing);

    let mut absent = fixture_doc();
    absent["fonts"][0]["sha256"] = json!("0".repeat(64));
    let err = render_with(absent.to_string().as_bytes(), "square", &s, opts).err().unwrap();
    assert_eq!(err[0].code, Code::FontMissing);
}

#[test]
fn report_records_font_backend() {
    let out = run(&fixture_doc(), "square", &store()).unwrap();
    assert_eq!(out.report.font_backend, Some(FontBackend::preferred()));
}

// ----------------------------------------------------------- showcase --

/// The showcase fixture (fictional brand, synthetic assets, two weights of
/// Noto Sans JP) must keep rendering without warnings and without any fit
/// reduction, and deterministically.
#[test]
fn showcase_fixtures_render_cleanly() {
    let cases = [
        ("showcase.json", "portrait", (1080, 1350), 2),
        ("showcase.json", "square", (1080, 1080), 2),
        ("retail-promo.json", "portrait", (1080, 1350), 2),
    ];
    for (file, variant, size, font_count) in cases {
        let doc: Value = serde_json::from_str(&fs::read_to_string(format!("{FIXTURES}/documents/{file}")).unwrap()).unwrap();
        let a = run(&doc, variant, &store()).unwrap();
        let b = run(&doc, variant, &store()).unwrap();
        assert_eq!(a.png, b.png, "{variant}");
        let p = pixels(&a.png);
        assert_eq!((p.0, p.1), size);
        assert!(a.report.warnings.is_empty(), "{variant}: {:?}", a.report.warnings);
        assert_eq!(a.report.fonts.len(), font_count);
        for t in &a.report.text {
            assert_eq!(t.applied_font_size, t.requested_font_size, "{variant}/{}", t.node_id);
        }
    }
}
