//! Structural and semantic validation tests for the core document model.
//! Mirrors the negative cases listed in validation/design_pack_checks.json.

use design_model::*;
use serde_json::{Value, json};

fn sample() -> Value {
    serde_json::from_str(include_str!("../../../examples/design.core.sample.json")).unwrap()
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/public-safe/documents/basic.json")).unwrap()
}

fn codes(v: &Value) -> Vec<Code> {
    match load_document(v.to_string().as_bytes()) {
        Ok(_) => vec![],
        Err(issues) => issues.into_iter().map(|i| i.code).collect(),
    }
}

fn node_mut<'a>(v: &'a mut Value, id: &str) -> &'a mut Value {
    v["nodes"].as_array_mut().unwrap().iter_mut().find(|n| n["id"] == id).unwrap()
}

#[test]
fn product_sample_is_structurally_valid() {
    // The sample is valid structure; it is NOT renderable (unresolved
    // assets/fonts, draft variants) — see design-render tests.
    assert_eq!(codes(&sample()), vec![]);
}

#[test]
fn fixture_document_is_valid() {
    assert_eq!(codes(&fixture()), vec![]);
}

#[test]
fn unknown_root_property_rejected() {
    let mut v = sample();
    v["extra"] = json!(1);
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn unknown_node_property_rejected() {
    let mut v = sample();
    node_mut(&mut v, "headline")["script"] = json!("alert(1)");
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn negative_canvas_size_rejected() {
    let mut v = sample();
    v["baseArtboard"]["width"] = json!(-1);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn oversized_canvas_dimension_rejected() {
    let mut v = sample();
    v["baseArtboard"]["height"] = json!(16385);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn invalid_opacity_rejected() {
    let mut v = sample();
    node_mut(&mut v, "photo")["opacity"] = json!(1.5);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn unsupported_node_type_rejected() {
    let mut v = sample();
    node_mut(&mut v, "background")["type"] = json!("script");
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn arbitrary_slot_property_rejected() {
    let mut v = sample();
    v["slots"][0]["target"] = json!("fill.color");
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn logo_aspect_distortion_rejected() {
    let mut v = sample();
    node_mut(&mut v, "brand-logo")["preserveAspectRatio"] = json!(false);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn unknown_schema_version_rejected() {
    let mut v = sample();
    v["schemaVersion"] = json!("0.3.0-core");
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn font_network_property_rejected() {
    let mut v = sample();
    v["fonts"][0]["url"] = json!("https://example.com/font.otf");
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn resolved_font_without_hash_rejected() {
    let mut v = sample();
    v["fonts"][0]["state"] = json!("resolved");
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn resolved_asset_without_hash_rejected() {
    let mut v = sample();
    v["assets"][0]["state"] = json!("resolved");
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn missing_nullable_hash_key_rejected() {
    let mut v = sample();
    v["assets"][0].as_object_mut().unwrap().remove("sha256");
    assert_eq!(codes(&v), vec![Code::Schema]);
}

#[test]
fn uppercase_hash_rejected() {
    let mut v = fixture();
    let h = v["fonts"][0]["sha256"].as_str().unwrap().to_uppercase();
    v["fonts"][0]["sha256"] = json!(h);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn zero_text_lines_rejected() {
    let mut v = sample();
    node_mut(&mut v, "headline")["text"]["maxLines"] = json!(0);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn duplicate_node_ids_rejected() {
    let mut v = sample();
    node_mut(&mut v, "photo")["id"] = json!("background");
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn missing_node_reference_rejected() {
    let mut v = sample();
    node_mut(&mut v, "root")["childIds"].as_array_mut().unwrap().push(json!("ghost"));
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn cycle_rejected() {
    let mut v = sample();
    // Turn "background" into a group that contains the root.
    let bg = node_mut(&mut v, "background");
    *bg = json!({
        "id": "background", "name": "g", "type": "group",
        "bounds": {"x": 0, "y": 0, "width": 10, "height": 10},
        "rotationDeg": 0, "opacity": 1, "visible": true, "locked": false,
        "childIds": ["root"]
    });
    let c = codes(&v);
    assert!(c.contains(&Code::DocumentInvalid), "{c:?}");
}

#[test]
fn wrong_slot_node_typing_rejected() {
    let mut v = sample();
    v["slots"][0]["nodeId"] = json!("photo");
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn missing_font_reference_rejected() {
    let mut v = sample();
    node_mut(&mut v, "headline")["text"]["fontId"] = json!("no-such-font");
    assert!(codes(&v).contains(&Code::FontMissing));
}

#[test]
fn unreachable_node_rejected() {
    let mut v = sample();
    node_mut(&mut v, "root")["childIds"]
        .as_array_mut()
        .unwrap()
        .retain(|c| c != "photo");
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn fit_sizes_must_be_descending() {
    let mut v = sample();
    node_mut(&mut v, "headline")["text"]["allowedFontSizes"] = json!([56, 60, 64]);
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn font_size_must_be_an_approved_size() {
    let mut v = sample();
    node_mut(&mut v, "headline")["text"]["fontSize"] = json!(70);
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn variant_font_size_patch_must_be_approved() {
    let mut v = sample();
    v["variants"][1]["nodePatches"][3]["fontSize"] = json!(40);
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn error_policy_must_not_list_sizes() {
    let mut v = fixture();
    node_mut(&mut v, "caption")["text"]["allowedFontSizes"] = json!([28, 24]);
    assert!(codes(&v).contains(&Code::DocumentInvalid));
}

#[test]
fn image_node_cannot_use_vector_asset() {
    let mut v = fixture();
    node_mut(&mut v, "photo")["assetId"] = json!("logo");
    assert!(codes(&v).contains(&Code::AssetType));
}

#[test]
fn oversized_document_rejected() {
    let big = vec![b' '; validate::MAX_DOCUMENT_BYTES + 1];
    assert_eq!(parse_document(&big).unwrap_err().code, Code::ResourceLimit);
}

#[test]
fn unknown_variant_is_layout_undefined() {
    let doc = load_document(fixture().to_string().as_bytes()).unwrap();
    assert_eq!(resolve_variant(&doc, "landscape").unwrap_err().code, Code::LayoutUndefined);
}

#[test]
fn draft_variant_is_not_exportable() {
    let doc = load_document(fixture().to_string().as_bytes()).unwrap();
    let v = resolve_variant(&doc, "wide-draft").unwrap();
    assert_eq!(require_approved(&v).unwrap_err().code, Code::VariantNotApproved);
    let v = resolve_variant(&doc, "square").unwrap();
    assert!(require_approved(&v).is_ok());
}

#[test]
fn variant_patches_apply_only_declared_fields() {
    let doc = load_document(fixture().to_string().as_bytes()).unwrap();
    let v = resolve_variant(&doc, "portrait").unwrap();
    let head = v.nodes.iter().find(|n| n.id() == "headline").unwrap();
    let Node::Text { bounds, text, fill, .. } = head else { panic!() };
    assert_eq!(bounds.y, 900.0);
    assert_eq!(text.font_size, 60.0);
    // Everything else is untouched.
    assert_eq!(fill.color, "#171717");
    assert_eq!(text.line_height, 80.0);
}
