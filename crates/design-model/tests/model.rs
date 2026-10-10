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
    v["schemaVersion"] = json!("0.5.0-core");
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

// ------------------------------------------------- 0.3.0-core additions --

fn template() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/public-safe/documents/retail-promo-template.json")).unwrap()
}

#[test]
fn template_is_valid_0_3() {
    assert_eq!(codes(&template()), vec![]);
}

#[test]
fn effects_require_schema_0_3() {
    let mut v = template();
    v["schemaVersion"] = json!("0.2.0-core");
    let c = codes(&v);
    assert!(c.contains(&Code::Schema), "{c:?}");
}

#[test]
fn effect_values_are_range_checked() {
    let mut v = template();
    node_mut(&mut v, "price")["outline"]["width"] = json!(0);
    assert!(codes(&v).contains(&Code::Schema));
    let mut v = template();
    node_mut(&mut v, "price")["shadow"]["opacity"] = json!(2);
    assert!(codes(&v).contains(&Code::Schema));
    let mut v = template();
    node_mut(&mut v, "price")["outline"]["dash"] = json!(3);
    assert!(codes(&v).contains(&Code::Schema));
}

#[test]
fn line_height_patch_only_on_text_in_0_3() {
    let mut v = template();
    v["variants"][1]["nodePatches"][0]["lineHeight"] = json!(40); // root group
    assert!(codes(&v).contains(&Code::Schema));
}

mod binding {
    use super::*;
    use std::collections::BTreeMap;

    fn doc() -> DesignDocument {
        load_document(template().to_string().as_bytes()).unwrap()
    }

    fn row(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn full() -> BTreeMap<String, String> {
        row(&[
            ("branch", "北町店"),
            ("price", "09,800"),
            ("fastest", "60分"),
            ("tel", "0120-000-101"),
            ("hours", "受付"),
        ])
    }

    fn text_of<'a>(d: &'a DesignDocument, id: &str) -> &'a str {
        match d.nodes.iter().find(|n| n.id() == id).unwrap() {
            Node::Text { text, .. } => &text.value,
            _ => panic!(),
        }
    }

    #[test]
    fn binds_only_declared_targets_verbatim() {
        let d = doc();
        let b = bind_row(&d, "r1", &full()).unwrap();
        assert_eq!(text_of(&b, "price"), "09,800"); // leading zero kept
        assert_eq!(text_of(&b, "tel"), "0120-000-101");
        // Everything that is not a slot target is unchanged.
        let changed: Vec<&str> = d.nodes.iter().zip(&b.nodes).filter(|(a, b)| a != b).map(|(a, _)| a.id()).collect();
        assert_eq!(changed, vec!["shop-sub", "price", "badge-main", "tel", "hours"]);
    }

    #[test]
    fn required_empty_unknown_and_too_long_are_refused() {
        let d = doc();
        let mut r = full();
        r.insert("tel".into(), String::new());
        r.insert("colour".into(), "red".into());
        r.insert("price".into(), "123,456,789".into());
        let e = bind_row(&d, "r9", &r).unwrap_err();
        assert_eq!(e.len(), 3, "{e:?}");
        assert!(
            e.iter()
                .all(|i| i.code == Code::DataInvalid && i.location.row_id.as_deref() == Some("r9"))
        );
    }
}

#[test]
fn advanced_styles_require_0_4_and_validate_references_and_limits() {
    let mut v = fixture();
    v["nodeStyles"] = json!([{"nodeId":"background","stroke":{"color":"#ffffff","width":2}}]);
    assert!(codes(&v).contains(&Code::Schema));
    v["schemaVersion"] = json!("0.4.0-core");
    assert!(codes(&v).is_empty());
    v["nodeStyles"][0]["nodeId"] = json!("missing");
    assert!(codes(&v).contains(&Code::DocumentInvalid));
    v["nodeStyles"][0]["nodeId"] = json!("headline");
    assert!(codes(&v).contains(&Code::DocumentInvalid));
    v["nodeStyles"][0] = json!({"nodeId":"background","blendMode":"overlay"});
    assert!(codes(&v).contains(&Code::Schema));
}
#[test]
fn text_run_ranges_use_utf8_boundaries_and_declared_fonts() {
    let mut v = fixture();
    v["schemaVersion"] = json!("0.4.0-core");
    let font = v["fonts"][0]["id"].clone();
    let n = node_mut(&mut v, "headline");
    n["text"]["value"] = json!("日本語ABC");
    n["text"]["runs"] = json!([{"start":0,"end":6,"fontId":font,"color":"#ff0000"}]);
    assert!(codes(&v).is_empty());
    node_mut(&mut v, "headline")["text"]["runs"][0]["start"] = json!(1);
    assert!(codes(&v).contains(&Code::DocumentInvalid));
    node_mut(&mut v, "headline")["text"]["runs"][0]["start"] = json!(0);
    node_mut(&mut v, "headline")["text"]["runs"][0]["fontId"] = json!("missing");
    assert!(codes(&v).contains(&Code::FontMissing));
}
#[test]
fn editable_path_parser_bounds_expanded_relative_coordinates() {
    assert!(design_svg::parse_path("M0 0 C10 20 30 40 50 60 Q60 70 80 90 Z").is_ok());
    assert!(design_svg::parse_path("M0 0 A10 10 0 0 0 20 20").is_err());
    assert!(design_svg::parse_path("M0 0 l1000000 0 l1000000 0").is_err());
    assert!(design_svg::parse_path("MNaN 0").is_err());
}

#[test]
fn brand_snapshot_preserves_named_styles_and_restricts_logo_choices() {
    let mut v = fixture();
    v["schemaVersion"] = json!("0.4.0-core");
    let text = node_mut(&mut v, "headline")["text"].clone();
    let logo = node_mut(&mut v, "brand-logo")["assetId"].clone();
    v["brandSnapshot"] = json!({"id":"test-brand","revision":1,"colors":[{"id":"accent","name":"Accent","color":"#39735b"}],"textStyles":[{"id":"headline","name":"Headline","style":text}],"logoAssetIds":[logo]});
    let doc = load_document(v.to_string().as_bytes()).unwrap();
    assert_eq!(doc.brand_snapshot.as_ref().unwrap().colors[0].name, "Accent");
    assert_eq!(load_document(&serde_json::to_vec(&doc).unwrap()).unwrap(), doc);
    v["brandSnapshot"]["logoAssetIds"] = json!([]);
    assert!(codes(&v).contains(&Code::AssetType));
    v["brandSnapshot"]["logoAssetIds"] = json!(["missing"]);
    assert!(codes(&v).contains(&Code::AssetMissing));
}
