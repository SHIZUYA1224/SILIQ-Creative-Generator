use design_model::{Code, load_document};
use design_render::resources::ResourceStore;
use design_render::{RenderOptions, render, render_preview_document};
use serde_json::{Value, json};
use std::path::Path;
fn store() -> ResourceStore {
    ResourceStore::open(Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/public-safe/resources"
    )))
    .unwrap()
}
fn rect(id: &str, color: &str, x: i32, y: i32, w: i32, h: i32) -> Value {
    json!({"id":id,"name":id,"type":"rect","bounds":{"x":x,"y":y,"width":w,"height":h},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"fill":{"kind":"solid","color":color},"cornerRadius":0})
}
fn doc() -> Value {
    json!({"format":"siliq-creative-generator-core","schemaVersion":"0.4.0-core","documentId":"test","revision":0,"title":"public-safe","colorSpace":"sRGB","baseArtboard":{"width":64,"height":64,"rootNodeId":"root"},"nodes":[{"id":"root","name":"root","type":"group","bounds":{"x":0,"y":0,"width":64,"height":64},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"childIds":["background","shape"]},rect("background","#808080",0,0,64,64),rect("shape","#ff0000",8,8,48,48)],"fonts":[],"assets":[],"slots":[],"variants":[{"id":"main","label":"test","width":64,"height":64,"approvalStatus":"approved","nodePatches":[]}]})
}
fn pixels(d: &Value) -> Vec<u8> {
    render(d.to_string().as_bytes(), "main", &store()).unwrap().rgba
}
fn px(p: &[u8], x: usize, y: usize) -> [u8; 4] {
    p[(y * 64 + x) * 4..(y * 64 + x) * 4 + 4].try_into().unwrap()
}

#[test]
fn editable_bezier_path_compound_fill_and_stroke_render() {
    let mut d = doc();
    d["nodes"][2] = json!({"id":"shape","name":"path","type":"path","bounds":{"x":8,"y":8,"width":48,"height":48},"viewBox":{"x":0,"y":0,"width":48,"height":48},"data":"M0 0 C16 0 32 0 48 0 L48 48 L0 48 Z M16 16 L32 16 L32 32 L16 32 Z","fillRule":"even-odd","rotationDeg":0,"opacity":1,"visible":true,"locked":false,"fill":{"kind":"solid","color":"#ff0000"}});
    d["nodeStyles"] = json!([{"nodeId":"shape","stroke":{"color":"#0000ff","width":4}}]);
    let p = pixels(&d);
    assert_eq!(px(&p, 12, 12), [255, 0, 0, 255]);
    assert_eq!(px(&p, 30, 30), [128, 128, 128, 255]);
    assert_eq!(px(&p, 8, 16), [0, 0, 255, 255]);
}
#[test]
fn radial_gradient_has_authored_center_and_radius() {
    let mut d = doc();
    d["nodes"][2]["fill"] = json!({"kind":"radial-gradient","center":{"x":0.5,"y":0.5},"radius":0.5,"stops":[{"offset":0,"color":"#ffffff","opacity":1},{"offset":1,"color":"#000000","opacity":1}]});
    let p = pixels(&d);
    assert!(px(&p, 32, 32)[0] > 240);
    assert!(px(&p, 9, 9)[0] < 8);
}
#[test]
fn multiply_and_screen_apply_to_the_authored_node_only() {
    let mut d = doc();
    d["nodeStyles"] = json!([{"nodeId":"shape","blendMode":"multiply"}]);
    let p = pixels(&d);
    assert_eq!(px(&p, 32, 32), [128, 0, 0, 255]);
    assert_eq!(px(&p, 0, 0), [128, 128, 128, 255]);
    d["nodeStyles"][0]["blendMode"] = json!("screen");
    assert_eq!(px(&pixels(&d), 32, 32), [255, 128, 128, 255]);
}
#[test]
fn clip_and_alpha_mask_preserve_the_background() {
    let mut d = doc();
    d["nodeStyles"] = json!([{"nodeId":"shape","clip":{"data":"M0 0 H32 V64 H0 Z","fillRule":"non-zero"}}]);
    let p = pixels(&d);
    assert_eq!(px(&p, 16, 32), [255, 0, 0, 255]);
    assert_eq!(px(&p, 48, 32), [128, 128, 128, 255]);
    d["nodeStyles"] = json!([{"nodeId":"shape","mask":{"data":"M0 0 H32 V64 H0 Z","fillRule":"non-zero","opacity":1,"invert":false}}]);
    let p = pixels(&d);
    assert_eq!(px(&p, 16, 32), [255, 0, 0, 255]);
    assert_eq!(px(&p, 48, 32), [128, 128, 128, 255]);
    d["nodeStyles"][0]["mask"]["invert"] = json!(true);
    let p = pixels(&d);
    assert_eq!(px(&p, 16, 32), [128, 128, 128, 255]);
    assert_eq!(px(&p, 48, 32), [255, 0, 0, 255]);
}
#[test]
fn shape_shadow_and_blur_are_real_pixel_effects() {
    let mut d = doc();
    d["nodes"][2] = rect("shape", "#ff0000", 8, 8, 16, 16);
    d["nodeStyles"] =
        json!([{"nodeId":"shape","effects":[{"kind":"shadow","color":"#000000","opacity":1,"offsetX":8,"offsetY":8,"blur":0}]}]);
    assert_eq!(px(&pixels(&d), 29, 29), [0, 0, 0, 255]);
    d["nodeStyles"][0]["effects"] = json!([{"kind":"blur","radius":5}]);
    let p = pixels(&d);
    assert!(px(&p, 7, 16)[0] > 128);
    assert!(px(&p, 16, 16)[0] < 255);
}
#[test]
fn preview_accepts_draft_but_production_export_refuses_it() {
    let mut d = doc();
    d["variants"][0]["approvalStatus"] = json!("draft");
    let model = load_document(d.to_string().as_bytes()).unwrap();
    assert_eq!(
        render(d.to_string().as_bytes(), "main", &store())
            .err()
            .unwrap()
            .first()
            .unwrap()
            .code,
        Code::VariantNotApproved
    );
    let out = render_preview_document(&model, "main", &store(), RenderOptions::default()).unwrap();
    assert_eq!(out.rgba.len(), 64 * 64 * 4);
}
#[test]
fn strict_japanese_kinsoku_preserves_original_text_and_report_offsets() {
    let mut d: Value = serde_json::from_str(include_str!("../../../fixtures/public-safe/documents/basic.json")).unwrap();
    d["schemaVersion"] = json!("0.4.0-core");
    let headline = d["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n["id"] == "headline")
        .unwrap();
    headline["bounds"]["width"] = json!(200);
    headline["bounds"]["height"] = json!(300);
    headline["text"] = json!({"value":"あいうえおーかきくけこ","fontId":"font-main","fontSize":40,"lineHeight":50,"letterSpacing":0,"maxLines":6,"overflowPolicy":"error","allowedFontSizes":[],"language":"ja","align":"left","kinsoku":"strict-japanese"});
    // The actual fixture font ID is kept rather than inventing a font resource.
    let font_id = d["fonts"][0]["id"].clone();
    let headline = d["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n["id"] == "headline")
        .unwrap();
    headline["text"]["fontId"] = font_id;
    let original = headline["text"]["value"].as_str().unwrap().to_owned();
    let out = render(d.to_string().as_bytes(), "square", &store()).unwrap();
    let report = out.report.text.iter().find(|t| t.node_id == "headline").unwrap();
    for line in &report.lines {
        assert!(original.is_char_boundary(line.start));
        assert!(original.is_char_boundary(line.end));
        assert!(!original[line.start..line.end].starts_with('ー'));
    }
}

#[test]
fn author_declared_logo_clearspace_is_preflighted() {
    let mut d = doc();
    d["assets"] = json!([{"id":"photo","kind":"raster","state":"resolved","sha256":"f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3d9a1995bde5cda0","mimeHint":"image/png"}]);
    d["nodes"][2] = json!({"id":"logo","name":"logo","type":"logo","bounds":{"x":16,"y":16,"width":24,"height":24},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"assetId":"photo","fit":"contain","preserveAspectRatio":true,"minWidth":1,"clearSpace":4});
    d["nodes"].as_array_mut().unwrap().push(rect("target", "#ff0000", 0, 0, 16, 64));
    d["nodes"][0]["childIds"] = json!(["background", "logo", "target"]);
    d["nodeStyles"] = json!([{"nodeId":"logo","protectedFrom":["target"]}]);
    let errors = render(d.to_string().as_bytes(), "main", &store()).err().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e.code == Code::LogoClearSpace && e.location.node_id.as_deref() == Some("logo"))
    );
    d["nodes"][3]["bounds"]["width"] = json!(8);
    assert!(render(d.to_string().as_bytes(), "main", &store()).is_ok());
    d["nodes"][2]["clearSpace"] = json!(0);
    d["nodeStyles"] = json!([{"nodeId":"root","affine":[2,0,0,1,0,0]}]);
    assert!(
        render(d.to_string().as_bytes(), "main", &store())
            .err()
            .unwrap()
            .iter()
            .any(|e| { e.code == Code::DocumentInvalid && e.location.node_id.as_deref() == Some("logo") })
    );
    d["nodes"][2]["minWidth"] = json!(20);
    d["nodeStyles"][0]["affine"] = json!([0.5, 0, 0, 0.5, 0, 0]);
    assert!(
        render(d.to_string().as_bytes(), "main", &store())
            .err()
            .unwrap()
            .iter()
            .any(|e| { e.code == Code::LogoMinWidth && e.location.node_id.as_deref() == Some("logo") })
    );
}
#[test]
fn image_adjustments_and_protected_crop_are_explicit() {
    let mut d = doc();
    d["assets"] = json!([{"id":"photo","kind":"raster","state":"resolved","sha256":"f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3d9a1995bde5cda0","mimeHint":"image/png"}]);
    d["nodes"][2] = json!({"id":"shape","name":"photo","type":"image","bounds":{"x":8,"y":8,"width":48,"height":48},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"assetId":"photo","fit":"cover","focalPoint":{"x":0.5,"y":0.5},"clipToBounds":true});
    d["nodeStyles"] = json!([{"nodeId":"shape","imageAdjustments":{"brightness":0,"contrast":1,"saturation":0}}]);
    let p = pixels(&d);
    let pixel = px(&p, 30, 30);
    assert!(pixel[0].abs_diff(pixel[1]) <= 1 && pixel[1].abs_diff(pixel[2]) <= 1);
    d["nodeStyles"][0]["protectedRegion"] = json!({"x":0,"y":0,"width":0.1,"height":1});
    assert!(render(d.to_string().as_bytes(), "main", &store()).is_err());
    d["nodes"][2]["fit"] = json!("contain");
    assert!(render(d.to_string().as_bytes(), "main", &store()).is_ok());
}
#[test]
fn mixed_runs_use_only_declared_faces_and_keep_offsets() {
    let mut d: Value = serde_json::from_str(include_str!("../../../fixtures/public-safe/documents/basic.json")).unwrap();
    d["schemaVersion"] = json!("0.4.0-core");
    let font = d["fonts"][0]["id"].clone();
    let n = d["nodes"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|n| n["id"] == "headline")
        .unwrap();
    n["text"]["value"] = json!("日本語ABC");
    n["text"]["runs"] = json!([{"start":0,"end":9,"fontId":font,"color":"#ff0000"}]);
    n["text"]["features"] = json!([{"tag":"kern","value":1}]);
    let out = render(d.to_string().as_bytes(), "square", &store()).unwrap();
    assert!(
        out.report
            .text
            .iter()
            .find(|t| t.node_id == "headline")
            .unwrap()
            .lines
            .iter()
            .all(|l| l.end <= 12)
    );
}
#[test]
fn webp_decode_applies_exif_once_and_rejects_invalid_icc() {
    use design_model::MimeHint;
    use skia_safe::{AlphaType, ColorType, ImageInfo};
    let input = [
        255u8, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255, 0, 0, 0, 255, 255, 255, 0, 255,
    ];
    // TIFF little endian, one SHORT orientation field, value 6 (90 degrees CW).
    let exif = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut bytes = Vec::new();
    let mut encoder = image_webp::WebPEncoder::new(&mut bytes);
    encoder.set_exif_metadata(exif.to_vec());
    encoder.set_icc_profile(design_render::resources::SRGB_ICC.to_vec());
    encoder.encode(&input, 3, 2, image_webp::ColorType::Rgba8).unwrap();
    let (image, info) = design_render::raster::decode(&bytes, MimeHint::Webp).unwrap();
    assert_eq!((info.width, info.height, info.orientation_applied), (2, 3, 6));
    let mut p = vec![0; 24];
    assert!(image.read_pixels(
        &ImageInfo::new((2, 3), ColorType::RGBA8888, AlphaType::Unpremul, None),
        &mut p,
        8,
        (0, 0),
        skia_safe::image::CachingHint::Disallow
    ));
    assert_eq!(&p[0..4], &input[12..16]);
    assert_eq!(&p[4..8], &input[0..4]);
    let mut bytes = Vec::new();
    let mut encoder = image_webp::WebPEncoder::new(&mut bytes);
    encoder.set_icc_profile(b"invalid profile".to_vec());
    encoder.encode(&input, 3, 2, image_webp::ColorType::Rgba8).unwrap();
    assert!(design_render::raster::decode(&bytes, MimeHint::Webp).is_err());
}

#[test]
fn group_affine_resize_moves_real_pixels_without_rewriting_child_layout() {
    use design_edit::{Command, Mode, Session};
    use design_model::Bounds;
    let mut d = doc();
    d["nodes"][0]["childIds"] = json!(["background", "group"]);
    d["nodes"][2] = rect("shape", "#ff0000", 8, 8, 16, 16);
    d["nodes"].as_array_mut().unwrap().push(json!({"id":"group","name":"group","type":"group","bounds":{"x":8,"y":8,"width":16,"height":16},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"childIds":["shape"]}));
    let model = load_document(d.to_string().as_bytes()).unwrap();
    let mut session = Session::new(model, Mode::Authoring).unwrap();
    let before = session.document().nodes[2].clone();
    session
        .apply(
            session.revision(),
            &Command::SetBounds {
                node_id: "group".into(),
                bounds: Bounds {
                    x: 16.0,
                    y: 16.0,
                    width: 32.0,
                    height: 32.0,
                },
            },
        )
        .unwrap();
    assert_eq!(session.document().nodes[2], before);
    let out = render_preview_document(session.document(), "main", &store(), RenderOptions::default()).unwrap();
    assert_eq!(px(&out.rgba, 32, 32), [255, 0, 0, 255]);
    assert_eq!(px(&out.rgba, 8, 8), [128, 128, 128, 255]);
    assert_eq!(px(&out.rgba, 47, 47), [255, 0, 0, 255]);
    // A variant's explicit group affine overrides the base style, without inferred resizing.
    let mut variant = session.document().clone();
    variant.variants[0].approval_status = design_model::ApprovalStatus::Approved;
    variant.variants[0].node_patches.push(
        serde_json::from_value(json!({"nodeId":"group","bounds":{"x":8,"y":8,"width":16,"height":16},"affine":[1,0,0,1,0,0]})).unwrap(),
    );
    let out = design_render::render_document(&variant, "main", &store(), RenderOptions::default()).unwrap();
    assert_eq!(px(&out.rgba, 12, 12), [255, 0, 0, 255]);
    assert_eq!(px(&out.rgba, 32, 32), [128, 128, 128, 255]);
}
#[test]
fn aggregate_reservation_stops_many_valid_images_before_pixel_decode() {
    let mut d = doc();
    d["nodes"].as_array_mut().unwrap().truncate(2);
    d["nodes"][0]["childIds"] = json!(["background"]);
    for i in 0..200 {
        let id = format!("photo-{i}");
        let asset = format!("asset-{i}");
        d["nodes"].as_array_mut().unwrap().push(json!({"id":id,"name":"image","type":"image","bounds":{"x":0,"y":0,"width":64,"height":64},"rotationDeg":0,"opacity":1,"visible":true,"locked":false,"assetId":asset,"fit":"contain","focalPoint":{"x":0.5,"y":0.5},"clipToBounds":true}));
        d["nodes"][0]["childIds"].as_array_mut().unwrap().push(json!(id));
        d["assets"].as_array_mut().unwrap().push(json!({"id":asset,"kind":"raster","state":"resolved","sha256":"f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3a1995bde5cda0","mimeHint":"image/png"}));
    }
    // Actual safe fixture hash. Each alias would otherwise receive an independently decoded image.
    for asset in d["assets"].as_array_mut().unwrap() {
        asset["sha256"] = json!("f2f5342afd08ccb89ec325245f75069eb1936bd9fc3fdf9d3d9a1995bde5cda0");
    }
    let errors = render(d.to_string().as_bytes(), "main", &store()).err().unwrap();
    assert!(
        errors
            .iter()
            .any(|e| e.code == Code::ResourceLimit && e.message.contains("estimated native working set"))
    );
}
#[test]
fn offscreen_layer_and_output_surfaces_share_one_reservation() {
    let mut d = doc();
    d["baseArtboard"]["width"] = json!(8192);
    d["baseArtboard"]["height"] = json!(8192);
    d["variants"][0]["width"] = json!(8192);
    d["variants"][0]["height"] = json!(8192);
    d["nodes"][0]["opacity"] = json!(0.5);
    let errors = render(d.to_string().as_bytes(), "main", &store()).err().unwrap();
    assert!(errors.iter().any(|e| e.code == Code::ResourceLimit));
}
