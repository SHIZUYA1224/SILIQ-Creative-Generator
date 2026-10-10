//! Edit session: transactions, revisions, Undo/Redo, locks, production mode.

use design_edit::{Command, Mode, Session, TextStylePatch};
use design_model::{Bounds, Code, DesignDocument, Node, load_document};
use serde_json::json;

fn doc() -> DesignDocument {
    load_document(include_bytes!("../../../fixtures/public-safe/documents/retail-promo-template.json")).unwrap()
}

fn node<'a>(d: &'a DesignDocument, id: &str) -> &'a Node {
    d.nodes.iter().find(|n| n.id() == id).unwrap()
}

fn text(d: &DesignDocument, id: &str) -> String {
    match node(d, id) {
        Node::Text { text, .. } => text.value.clone(),
        _ => panic!(),
    }
}

fn codes(e: Vec<design_model::Issue>) -> Vec<Code> {
    e.into_iter().map(|i| i.code).collect()
}

#[test]
fn apply_undo_redo_with_monotonic_revisions() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let r0 = s.revision();
    let r1 = s
        .apply(
            r0,
            &Command::SetText {
                node_id: "price".into(),
                value: "7,980".into(),
            },
        )
        .unwrap();
    assert_eq!(r1, r0 + 1);
    assert_eq!(text(s.document(), "price"), "7,980");
    let r2 = s.undo(r1).unwrap();
    assert_eq!(text(s.document(), "price"), "9,800");
    let r3 = s.redo(r2).unwrap();
    assert_eq!(text(s.document(), "price"), "7,980");
    assert!(r3 > r2 && r2 > r1);
    // A new edit clears the redo stack.
    s.undo(r3).unwrap();
    s.apply(
        s.revision(),
        &Command::SetText {
            node_id: "price".into(),
            value: "6,980".into(),
        },
    )
    .unwrap();
    assert!(!s.can_redo());
}

#[test]
fn batch_is_one_undo_step() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let before = s.document().clone();
    let b = |y: f64| Bounds {
        x: 60.0,
        y,
        width: 300.0,
        height: 128.0,
    };
    let cmd = Command::Batch {
        commands: vec![
            Command::SetBounds {
                node_id: "card-0".into(),
                bounds: b(840.0),
            },
            Command::SetBounds {
                node_id: "card-1".into(),
                bounds: b(840.0),
            },
        ],
    };
    let r = s.apply(s.revision(), &cmd).unwrap();
    s.undo(r).unwrap();
    assert_eq!(s.document().nodes, before.nodes);
}

#[test]
fn stale_revision_is_a_conflict_and_changes_nothing() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let r0 = s.revision();
    s.apply(
        r0,
        &Command::SetText {
            node_id: "price".into(),
            value: "1".into(),
        },
    )
    .unwrap();
    let e = s
        .apply(
            r0,
            &Command::SetText {
                node_id: "price".into(),
                value: "2".into(),
            },
        )
        .unwrap_err();
    assert_eq!(codes(e), vec![Code::RevisionConflict]);
    assert_eq!(text(s.document(), "price"), "1");
}

#[test]
fn invalid_results_are_rejected() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let r = s.revision();
    let e = s
        .apply(
            r,
            &Command::SetOpacity {
                node_id: "price".into(),
                opacity: 2.0,
            },
        )
        .unwrap_err();
    assert!(codes(e).contains(&Code::Schema));
    let e = s
        .apply(
            r,
            &Command::SetTextStyle {
                node_id: "price".into(),
                style: TextStylePatch {
                    font_size: Some(123.0),
                    ..Default::default()
                },
            },
        )
        .unwrap_err();
    assert!(codes(e).contains(&Code::DocumentInvalid)); // not an approved fit size
    assert_eq!(s.revision(), r);
}

#[test]
fn locked_nodes_cannot_change_until_unlocked() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let r = s.revision();
    let b = Bounds {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };
    let e = s
        .apply(
            r,
            &Command::SetBounds {
                node_id: "logo".into(),
                bounds: b,
            },
        )
        .unwrap_err();
    assert_eq!(codes(e), vec![Code::EditForbidden]);
    let r = s
        .apply(
            r,
            &Command::SetLocked {
                node_id: "logo".into(),
                locked: false,
            },
        )
        .unwrap();
    s.apply(
        r,
        &Command::SetBounds {
            node_id: "logo".into(),
            bounds: b,
        },
    )
    .unwrap();
}

#[test]
fn production_mode_only_changes_slots() {
    let mut s = Session::new(doc(), Mode::Production).unwrap();
    let r = s.revision();
    let r = s
        .apply(
            r,
            &Command::SetText {
                node_id: "price".into(),
                value: "8,800".into(),
            },
        )
        .unwrap();
    let e = s
        .apply(
            r,
            &Command::SetText {
                node_id: "catch".into(),
                value: "x".into(),
            },
        )
        .unwrap_err();
    assert_eq!(codes(e), vec![Code::EditForbidden]);
    let e = s
        .apply(
            r,
            &Command::SetOpacity {
                node_id: "price".into(),
                opacity: 0.5,
            },
        )
        .unwrap_err();
    assert_eq!(codes(e), vec![Code::EditForbidden]);
    let e = s
        .apply(
            r,
            &Command::SetText {
                node_id: "price".into(),
                value: "12,345,678".into(),
            },
        )
        .unwrap_err();
    assert_eq!(codes(e), vec![Code::DataInvalid]); // slot maxLength
}

#[test]
fn add_reorder_delete() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let r = s.revision();
    let n: Node = serde_json::from_value(json!({
        "id": "sticker", "name": "sticker", "type": "rect",
        "bounds": {"x": 10, "y": 10, "width": 50, "height": 50},
        "rotationDeg": 0, "opacity": 1, "visible": true, "locked": false,
        "fill": {"kind": "solid", "color": "#FF0000"}, "cornerRadius": 0
    }))
    .unwrap();
    let r = s
        .apply(
            r,
            &Command::AddNode {
                parent_id: "root".into(),
                index: 999,
                node: n.clone(),
            },
        )
        .unwrap();
    let r = s
        .apply(
            r,
            &Command::Reorder {
                node_id: "sticker".into(),
                index: 0,
            },
        )
        .unwrap();
    match node(s.document(), "root") {
        Node::Group { child_ids, .. } => assert_eq!(child_ids[0], "sticker"),
        _ => panic!(),
    }
    // Duplicate id refused; slot targets cannot be deleted.
    assert!(
        s.apply(
            r,
            &Command::AddNode {
                parent_id: "root".into(),
                index: 0,
                node: n
            }
        )
        .is_err()
    );
    assert!(s.apply(r, &Command::DeleteNode { node_id: "price".into() }).is_err());
    s.apply(r, &Command::DeleteNode { node_id: "sticker".into() }).unwrap();
    assert!(s.document().nodes.iter().all(|n| n.id() != "sticker"));
}

#[test]
fn commands_round_trip_as_json() {
    let c: Command = serde_json::from_value(json!({"op": "setText", "nodeId": "price", "value": "9,800"})).unwrap();
    assert_eq!(
        c,
        Command::SetText {
            node_id: "price".into(),
            value: "9,800".into()
        }
    );
    assert!(serde_json::from_value::<Command>(json!({"op": "runScript", "src": "x"})).is_err());
    assert!(serde_json::from_value::<Command>(json!({"op": "setText", "nodeId": "a", "value": "b", "extra": 1})).is_err());
}

#[test]
fn authoring_invalidates_approval_until_explicit_reapproval() {
    use design_model::ApprovalStatus;
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    s.apply(
        s.revision(),
        &Command::SetTitle {
            title: "new authored revision".into(),
        },
    )
    .unwrap();
    assert!(s.document().variants.iter().all(|v| v.approval_status == ApprovalStatus::Draft));
    let id = s.document().variants[0].id.clone();
    s.apply(
        s.revision(),
        &Command::SetVariantApproval {
            variant_id: id.clone(),
            approval_status: ApprovalStatus::Approved,
        },
    )
    .unwrap();
    assert_eq!(
        s.document().variants.iter().find(|v| v.id == id).unwrap().approval_status,
        ApprovalStatus::Approved
    );
    let r = s.revision();
    s.set_mode(r, Mode::Production).unwrap();
    assert!(!s.can_undo());
    let before = s.document().clone();
    assert!(s.apply(s.revision(), &Command::SetTitle { title: "forbidden".into() }).is_err());
    assert_eq!(s.document(), &before);
    s.set_mode(s.revision(), Mode::Authoring).unwrap();
    assert!(s.can_undo());
}
#[test]
fn malformed_path_edit_is_atomic_and_undo_keeps_old_path() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let path:Node=serde_json::from_value(json!({"id":"bezier","name":"curve","type":"path","bounds":{"x":0,"y":0,"width":100,"height":100},"viewBox":{"x":0,"y":0,"width":100,"height":100},"data":"M0 0 Q50 100 100 0 Z","fillRule":"non-zero","rotationDeg":0,"opacity":1,"visible":true,"locked":false,"fill":{"kind":"solid","color":"#ff0000"}})).unwrap();
    let root = s.document().base_artboard.root_node_id.clone();
    s.apply(
        s.revision(),
        &Command::AddNode {
            parent_id: root,
            index: 0,
            node: path,
        },
    )
    .unwrap();
    let before = s.document().clone();
    assert!(
        s.apply(
            s.revision(),
            &Command::SetPathData {
                node_id: "bezier".into(),
                data: "M0 0 A100 100 0 0 0 10 10".into()
            }
        )
        .is_err()
    );
    assert_eq!(s.document(), &before);
    s.apply(
        s.revision(),
        &Command::SetPathData {
            node_id: "bezier".into(),
            data: "M0 0 C10 20 80 20 100 0 Z".into(),
        },
    )
    .unwrap();
    s.undo(s.revision()).unwrap();
    assert_eq!(s.document().nodes, before.nodes);
}

#[test]
fn production_history_cannot_restore_fixed_authoring_properties() {
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    s.apply(
        s.revision(),
        &Command::SetOpacity {
            node_id: "price".into(),
            opacity: 0.5,
        },
    )
    .unwrap();
    s.set_mode(s.revision(), Mode::Production).unwrap();
    let before = s.document().clone();
    assert!(!s.can_undo());
    assert!(s.undo(s.revision()).is_err());
    assert_eq!(s.document(), &before);
    s.apply(
        s.revision(),
        &Command::SetText {
            node_id: "price".into(),
            value: "1,000".into(),
        },
    )
    .unwrap();
    assert!(s.can_undo());
    s.undo(s.revision()).unwrap();
    assert!(!s.can_undo());
    s.set_mode(s.revision(), Mode::Authoring).unwrap();
    assert!(s.can_undo());
    s.undo(s.revision()).unwrap();
    assert_eq!(node(s.document(), "price").common().2, 1.0);
}
#[test]
fn approval_at_the_end_of_an_atomic_authoring_batch_is_explicit() {
    use design_model::ApprovalStatus;
    let mut s = Session::new(doc(), Mode::Authoring).unwrap();
    let variant = s.document().variants[0].id.clone();
    s.apply(
        s.revision(),
        &Command::Batch {
            commands: vec![
                Command::SetTitle {
                    title: "confirmed artwork".into(),
                },
                Command::SetVariantApproval {
                    variant_id: variant.clone(),
                    approval_status: ApprovalStatus::Approved,
                },
            ],
        },
    )
    .unwrap();
    assert_eq!(
        s.document().variants.iter().find(|v| v.id == variant).unwrap().approval_status,
        ApprovalStatus::Approved
    );
}

#[test]
fn group_ungroup_preserves_node_ids_order_and_one_step_undo() {
    let mut d = doc();
    for node in &mut d.nodes {
        *node.common_mut().locked = false;
    }
    let root = d.base_artboard.root_node_id.clone();
    let ids = match node(&d, &root) {
        Node::Group { child_ids, .. } => child_ids[0..2].to_vec(),
        _ => panic!(),
    };
    let original = d.nodes.clone();
    let mut session = Session::new(d, Mode::Authoring).unwrap();
    session
        .apply(
            session.revision(),
            &Command::GroupNodes {
                node_ids: ids,
                group_id: "new-group".into(),
                name: "Named group".into(),
            },
        )
        .unwrap();
    assert!(session.document().nodes.iter().any(|n| n.id() == "new-group"));
    session
        .apply(
            session.revision(),
            &Command::UngroupNodes {
                group_id: "new-group".into(),
            },
        )
        .unwrap();
    assert_eq!(session.document().nodes, original);
    session.undo(session.revision()).unwrap();
    assert!(session.document().nodes.iter().any(|n| n.id() == "new-group"));
    session.undo(session.revision()).unwrap();
    assert_eq!(session.document().nodes, original);
}
#[test]
fn group_transform_refuses_locked_descendants_and_nonadjacent_grouping() {
    let mut d = doc();
    let root = d.base_artboard.root_node_id.clone();
    let child = match node(&d, &root) {
        Node::Group { child_ids, .. } => child_ids[0].clone(),
        _ => panic!(),
    };
    *d.nodes.iter_mut().find(|n| n.id() == child).unwrap().common_mut().locked = true;
    let mut session = Session::new(d, Mode::Authoring).unwrap();
    let before = session.document().clone();
    assert!(
        session
            .apply(
                session.revision(),
                &Command::SetBounds {
                    node_id: root.clone(),
                    bounds: Bounds {
                        x: 10.0,
                        y: 10.0,
                        width: 1080.0,
                        height: 1080.0
                    }
                }
            )
            .is_err()
    );
    assert_eq!(session.document(), &before);
    let mut style = design_model::NodeStyle::new(&root);
    style.affine = Some([1.0, 0.0, 0.0, 1.0, 10.0, 0.0]);
    for command in [
        Command::SetRotation {
            node_id: root.clone(),
            rotation_deg: 30.0,
        },
        Command::SetNodeStyle {
            node_id: root.clone(),
            style,
        },
    ] {
        assert!(session.apply(session.revision(), &command).is_err());
        assert_eq!(session.document(), &before);
    }
    let mut variant = session.document().variants[0].clone();
    variant.node_patches.push(
        serde_json::from_value(json!({
        "nodeId": root,
        "bounds": node(session.document(), &root).common().0,
            "affine": [1, 0, 0, 1, 10, 0]
        }))
        .unwrap(),
    );
    assert!(session.apply(session.revision(), &Command::SetVariant { variant }).is_err());
    assert_eq!(session.document(), &before);
    let mut d = doc();
    for node in &mut d.nodes {
        *node.common_mut().locked = false;
    }
    let ids = match node(&d, &root) {
        Node::Group { child_ids, .. } => vec![child_ids[0].clone(), child_ids[2].clone()],
        _ => panic!(),
    };
    let mut session = Session::new(d, Mode::Authoring).unwrap();
    assert!(
        session
            .apply(
                session.revision(),
                &Command::GroupNodes {
                    node_ids: ids,
                    group_id: "gap-group".into(),
                    name: "gap".into()
                }
            )
            .is_err()
    );
}

#[test]
fn replacing_imported_catalog_entries_keeps_their_positions() {
    let mut session = Session::new(doc(), Mode::Authoring).unwrap();
    let fonts: Vec<_> = session.document().fonts.iter().map(|f| f.id.clone()).collect();
    let assets: Vec<_> = session.document().assets.iter().map(|a| a.id.clone()).collect();
    let mut font = session.document().fonts[0].clone();
    font.name_hint = "Resolved font".into();
    let mut asset = session.document().assets[0].clone();
    asset.sha256 = Some("a".repeat(64));
    session.apply(session.revision(), &Command::ImportFont { font }).unwrap();
    session.apply(session.revision(), &Command::ImportAsset { asset }).unwrap();
    assert_eq!(session.document().fonts.iter().map(|f| f.id.clone()).collect::<Vec<_>>(), fonts);
    assert_eq!(session.document().assets.iter().map(|a| a.id.clone()).collect::<Vec<_>>(), assets);
}

#[test]
fn logo_asset_selection_is_authoring_only_and_keeps_asset_brand_and_geometry_guards() {
    use design_model::{BrandSnapshot, SCHEMA_VERSION};
    let mut d = load_document(include_bytes!("../../../fixtures/public-safe/documents/basic.json")).unwrap();
    let logo = d.nodes.iter_mut().find(|n| n.id() == "brand-logo").unwrap();
    *logo.common_mut().locked = false;
    let geometry = logo.clone();
    let mut alternative = d.assets.iter().find(|a| a.id == "logo").unwrap().clone();
    alternative.id = "alternative-logo".into();
    d.assets.push(alternative);
    let change = Command::SetImageAsset {
        node_id: "brand-logo".into(),
        asset_id: "alternative-logo".into(),
    };
    let mut session = Session::new(d.clone(), Mode::Authoring).unwrap();
    session.apply(session.revision(), &change).unwrap();
    let next = node(session.document(), "brand-logo");
    match (&geometry, next) {
        (
            Node::Logo {
                bounds,
                fit,
                preserve_aspect_ratio,
                min_width,
                clear_space,
                ..
            },
            Node::Logo {
                asset_id,
                bounds: b,
                fit: f,
                preserve_aspect_ratio: p,
                min_width: m,
                clear_space: c,
                ..
            },
        ) => {
            assert_eq!(asset_id, "alternative-logo");
            assert_eq!(bounds, b);
            assert_eq!(fit, f);
            assert_eq!(preserve_aspect_ratio, p);
            assert_eq!(min_width, m);
            assert_eq!(clear_space, c);
        }
        _ => panic!("fixture logo"),
    }
    let before = session.document().clone();
    assert!(
        session
            .apply(
                session.revision(),
                &Command::SetImageAsset {
                    node_id: "photo".into(),
                    asset_id: "alternative-logo".into()
                }
            )
            .is_err()
    );
    assert!(
        session
            .apply(
                session.revision(),
                &Command::SetImageAsset {
                    node_id: "brand-logo".into(),
                    asset_id: "missing-logo".into()
                }
            )
            .is_err()
    );
    assert_eq!(session.document(), &before);
    let mut production = Session::new(d.clone(), Mode::Production).unwrap();
    let before = production.document().clone();
    assert!(codes(production.apply(production.revision(), &change).unwrap_err()).contains(&Code::EditForbidden));
    assert_eq!(production.document(), &before);
    d.schema_version = SCHEMA_VERSION.into();
    d.brand_snapshot = Some(BrandSnapshot {
        id: "brand".into(),
        revision: 1,
        colors: vec![],
        text_styles: vec![],
        logo_asset_ids: vec!["logo".into()],
    });
    let mut branded = Session::new(d, Mode::Authoring).unwrap();
    let before = branded.document().clone();
    assert!(branded.apply(branded.revision(), &change).is_err());
    assert_eq!(branded.document(), &before);
}
