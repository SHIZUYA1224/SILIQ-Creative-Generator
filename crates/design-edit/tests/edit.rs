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
