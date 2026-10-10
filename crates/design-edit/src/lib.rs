//! Edit session (EDT-004, TPL-001/002): typed commands applied as
//! transactions on the canonical document, with revisions and Undo/Redo.
//!
//! - Every command carries the revision it was based on; a stale revision is
//!   `E_REVISION_CONFLICT` and nothing changes.
//! - A command (including a `batch` of sub-commands, e.g. one drag that moved
//!   several nodes) is one transaction and one Undo step.
//! - The result must pass full document validation, otherwise the command
//!   is rejected and the document is unchanged.
//! - Locked nodes (or nodes under a locked group) cannot be changed; only
//!   `setLocked` in authoring mode can unlock them.
//! - In production mode only slot targets can be changed (text of text
//!   slots, asset of image slots).
//!
//! Undo/Redo create new revisions (revisions only increase), so a UI can
//! always discard stale previews by revision.

use design_model::{
    Align, Bounds, Code, DesignDocument, Issue, Node, Paint, SlotTarget, SolidFill, SolidKind, TextOutline, TextShadow, has_errors,
    validate,
};
use serde::{Deserialize, Serialize};

pub const MAX_UNDO: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Design editing: everything except locked content.
    Authoring,
    /// Mass-production editing: only declared slot targets.
    Production,
}

/// Text style changes; absent fields stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TextStylePatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_height: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub letter_spacing: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_lines: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<Align>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    #[serde(rename_all = "camelCase")]
    SetBounds { node_id: String, bounds: Bounds },
    #[serde(rename_all = "camelCase")]
    SetRotation { node_id: String, rotation_deg: f64 },
    #[serde(rename_all = "camelCase")]
    SetOpacity { node_id: String, opacity: f64 },
    #[serde(rename_all = "camelCase")]
    SetVisible { node_id: String, visible: bool },
    #[serde(rename_all = "camelCase")]
    SetLocked { node_id: String, locked: bool },
    #[serde(rename_all = "camelCase")]
    SetText { node_id: String, value: String },
    #[serde(rename_all = "camelCase")]
    SetTextStyle { node_id: String, style: TextStylePatch },
    #[serde(rename_all = "camelCase")]
    SetTextEffects {
        node_id: String,
        outline: Option<TextOutline>,
        shadow: Option<TextShadow>,
    },
    #[serde(rename_all = "camelCase")]
    SetFill { node_id: String, fill: Paint },
    #[serde(rename_all = "camelCase")]
    SetImageAsset { node_id: String, asset_id: String },
    /// Move `node_id` to position `index` among its parent's children
    /// (0 = drawn first / bottom).
    #[serde(rename_all = "camelCase")]
    Reorder { node_id: String, index: usize },
    #[serde(rename_all = "camelCase")]
    AddNode { parent_id: String, index: usize, node: Node },
    /// Deletes the node and its whole subtree.
    #[serde(rename_all = "camelCase")]
    DeleteNode { node_id: String },
    /// Several commands applied as one transaction (one Undo step).
    Batch { commands: Vec<Command> },
}

#[derive(Debug, Clone)]
pub struct Session {
    doc: DesignDocument,
    mode: Mode,
    undo: Vec<DesignDocument>,
    redo: Vec<DesignDocument>,
}

fn forbid(msg: impl Into<String>) -> Issue {
    Issue::error(
        Code::EditForbidden,
        msg,
        "Unlock the node or switch to authoring mode if you are the author.",
    )
}

impl Session {
    /// Start a session on a valid document. The document's `revision` is the
    /// session's current revision.
    pub fn new(doc: DesignDocument, mode: Mode) -> Result<Session, Vec<Issue>> {
        let issues = validate(&doc);
        if has_errors(&issues) {
            return Err(issues);
        }
        Ok(Session {
            doc,
            mode,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    pub fn document(&self) -> &DesignDocument {
        &self.doc
    }

    pub fn revision(&self) -> u64 {
        self.doc.revision
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Apply one command as one transaction. Returns the new revision.
    pub fn apply(&mut self, expected_revision: u64, cmd: &Command) -> Result<u64, Vec<Issue>> {
        self.check_revision(expected_revision)?;
        let mut next = self.doc.clone();
        apply_command(&mut next, cmd, self.mode).map_err(|i| vec![i])?;
        let issues = validate(&next);
        if has_errors(&issues) {
            return Err(issues);
        }
        next.revision = self.doc.revision + 1;
        let prev = std::mem::replace(&mut self.doc, next);
        self.undo.push(prev);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
        Ok(self.doc.revision)
    }

    pub fn undo(&mut self, expected_revision: u64) -> Result<u64, Vec<Issue>> {
        self.check_revision(expected_revision)?;
        let Some(mut prev) = self.undo.pop() else {
            return Err(vec![Issue::error(Code::EditForbidden, "nothing to undo", "")]);
        };
        prev.revision = self.doc.revision + 1;
        let cur = std::mem::replace(&mut self.doc, prev);
        self.redo.push(cur);
        Ok(self.doc.revision)
    }

    pub fn redo(&mut self, expected_revision: u64) -> Result<u64, Vec<Issue>> {
        self.check_revision(expected_revision)?;
        let Some(mut next) = self.redo.pop() else {
            return Err(vec![Issue::error(Code::EditForbidden, "nothing to redo", "")]);
        };
        next.revision = self.doc.revision + 1;
        let cur = std::mem::replace(&mut self.doc, next);
        self.undo.push(cur);
        Ok(self.doc.revision)
    }

    fn check_revision(&self, expected: u64) -> Result<(), Vec<Issue>> {
        if expected != self.doc.revision {
            return Err(vec![Issue::error(
                Code::RevisionConflict,
                format!("edit was based on revision {expected}; the document is at {}", self.doc.revision),
                "Reload the document and repeat the edit.",
            )]);
        }
        Ok(())
    }
}

fn parent_of<'a>(doc: &'a DesignDocument, id: &str) -> Option<&'a str> {
    doc.nodes.iter().find_map(|n| match n {
        Node::Group { id: gid, child_ids, .. } if child_ids.iter().any(|c| c == id) => Some(gid.as_str()),
        _ => None,
    })
}

/// True when the node or any ancestor is locked.
fn effectively_locked(doc: &DesignDocument, id: &str) -> bool {
    let mut cur = Some(id);
    let mut guard = 0;
    while let Some(c) = cur {
        if doc.nodes.iter().find(|n| n.id() == c).is_some_and(|n| n.is_locked()) {
            return true;
        }
        cur = parent_of(doc, c);
        guard += 1;
        if guard > 4096 {
            break;
        }
    }
    false
}

fn node_mut<'a>(doc: &'a mut DesignDocument, id: &str) -> Result<&'a mut Node, Issue> {
    doc.nodes
        .iter_mut()
        .find(|n| n.id() == id)
        .ok_or_else(|| forbid(format!("node {id:?} does not exist")).node(id))
}

fn is_slot_target(doc: &DesignDocument, id: &str, target: SlotTarget) -> bool {
    doc.slots.iter().any(|s| s.node_id == id && s.target == target)
}

fn apply_command(doc: &mut DesignDocument, cmd: &Command, mode: Mode) -> Result<(), Issue> {
    // Permission check first.
    let (target, production_ok) = match cmd {
        Command::Batch { commands } => {
            for c in commands {
                apply_command(doc, c, mode)?;
            }
            return Ok(());
        }
        Command::SetText { node_id, .. } => (node_id.as_str(), is_slot_target(doc, node_id, SlotTarget::TextValue)),
        Command::SetImageAsset { node_id, .. } => (node_id.as_str(), is_slot_target(doc, node_id, SlotTarget::AssetId)),
        Command::AddNode { parent_id, .. } => (parent_id.as_str(), false),
        Command::SetBounds { node_id, .. }
        | Command::SetRotation { node_id, .. }
        | Command::SetOpacity { node_id, .. }
        | Command::SetVisible { node_id, .. }
        | Command::SetLocked { node_id, .. }
        | Command::SetTextStyle { node_id, .. }
        | Command::SetTextEffects { node_id, .. }
        | Command::SetFill { node_id, .. }
        | Command::Reorder { node_id, .. }
        | Command::DeleteNode { node_id } => (node_id.as_str(), false),
    };
    if mode == Mode::Production && !production_ok {
        return Err(Issue::error(
            Code::EditForbidden,
            "production mode only allows changing slot values",
            "Edit the template in authoring mode (this creates a new template revision).",
        )
        .node(target));
    }
    let unlocking = matches!(cmd, Command::SetLocked { locked: false, .. }) && mode == Mode::Authoring;
    if !unlocking && effectively_locked(doc, target) {
        return Err(forbid(format!("node {target:?} is locked")).node(target));
    }

    match cmd {
        Command::Batch { .. } => unreachable!(),
        Command::SetBounds { node_id, bounds } => *node_mut(doc, node_id)?.common_mut().bounds = *bounds,
        Command::SetRotation { node_id, rotation_deg } => *node_mut(doc, node_id)?.common_mut().rotation_deg = *rotation_deg,
        Command::SetOpacity { node_id, opacity } => *node_mut(doc, node_id)?.common_mut().opacity = *opacity,
        Command::SetVisible { node_id, visible } => *node_mut(doc, node_id)?.common_mut().visible = *visible,
        Command::SetLocked { node_id, locked } => *node_mut(doc, node_id)?.common_mut().locked = *locked,
        Command::SetText { node_id, value } => {
            if let Some(max) = doc
                .slots
                .iter()
                .filter(|s| &s.node_id == node_id)
                .filter_map(|s| s.max_length)
                .min()
                && value.chars().count() > max as usize
            {
                return Err(Issue::error(
                    Code::DataInvalid,
                    format!("text has {} characters; the slot allows {max}", value.chars().count()),
                    "Shorten the text.",
                )
                .node(node_id));
            }
            match node_mut(doc, node_id)? {
                Node::Text { text, .. } => text.value = value.clone(),
                _ => return Err(forbid("setText applies to text nodes").node(node_id)),
            }
        }
        Command::SetTextStyle { node_id, style } => match node_mut(doc, node_id)? {
            Node::Text { text, fill, .. } => {
                if let Some(v) = &style.font_id {
                    text.font_id = v.clone();
                }
                if let Some(v) = style.font_size {
                    text.font_size = v;
                }
                if let Some(v) = style.line_height {
                    text.line_height = v;
                }
                if let Some(v) = style.letter_spacing {
                    text.letter_spacing = v;
                }
                if let Some(v) = style.max_lines {
                    text.max_lines = v;
                }
                if let Some(v) = style.align {
                    text.align = v;
                }
                if let Some(v) = &style.color {
                    *fill = SolidFill {
                        kind: SolidKind::Solid,
                        color: v.clone(),
                    };
                }
            }
            _ => return Err(forbid("setTextStyle applies to text nodes").node(node_id)),
        },
        Command::SetTextEffects {
            node_id,
            outline: o,
            shadow: s,
        } => match node_mut(doc, node_id)? {
            Node::Text { outline, shadow, .. } => {
                *outline = o.clone();
                *shadow = s.clone();
            }
            _ => return Err(forbid("setTextEffects applies to text nodes").node(node_id)),
        },
        Command::SetFill { node_id, fill: f } => match node_mut(doc, node_id)? {
            Node::Rect { fill, .. } => *fill = f.clone(),
            _ => return Err(forbid("setFill applies to rect nodes").node(node_id)),
        },
        Command::SetImageAsset { node_id, asset_id: a } => match node_mut(doc, node_id)? {
            Node::Image { asset_id, .. } => *asset_id = a.clone(),
            _ => return Err(forbid("setImageAsset applies to image nodes").node(node_id)),
        },
        Command::Reorder { node_id, index } => {
            let parent = parent_of(doc, node_id)
                .ok_or_else(|| forbid("the root cannot be reordered").node(node_id))?
                .to_owned();
            if effectively_locked(doc, &parent) {
                return Err(forbid(format!("parent {parent:?} is locked")).node(node_id));
            }
            if let Node::Group { child_ids, .. } = node_mut(doc, &parent)? {
                let pos = child_ids.iter().position(|c| c == node_id).expect("child of parent");
                let id = child_ids.remove(pos);
                child_ids.insert((*index).min(child_ids.len()), id);
            }
        }
        Command::AddNode { parent_id, index, node } => {
            if doc.nodes.iter().any(|n| n.id() == node.id()) {
                return Err(forbid(format!("node id {:?} already exists", node.id())));
            }
            if matches!(node, Node::Group { child_ids, .. } if !child_ids.is_empty()) {
                return Err(forbid("add groups empty and add their children separately"));
            }
            match node_mut(doc, parent_id)? {
                Node::Group { child_ids, .. } => {
                    let i = (*index).min(child_ids.len());
                    child_ids.insert(i, node.id().to_owned());
                }
                _ => return Err(forbid("nodes can only be added to groups").node(parent_id)),
            }
            doc.nodes.push(node.clone());
        }
        Command::DeleteNode { node_id } => {
            let parent = parent_of(doc, node_id)
                .ok_or_else(|| forbid("the root cannot be deleted").node(node_id))?
                .to_owned();
            if effectively_locked(doc, &parent) {
                return Err(forbid(format!("parent {parent:?} is locked")).node(node_id));
            }
            // Collect the subtree; refuse if any part is locked or a slot target.
            let mut subtree = vec![node_id.clone()];
            let mut i = 0;
            while i < subtree.len() {
                if let Some(Node::Group { child_ids, .. }) = doc.nodes.iter().find(|n| n.id() == subtree[i]) {
                    subtree.extend(child_ids.iter().cloned());
                }
                i += 1;
            }
            for id in &subtree {
                if doc.nodes.iter().find(|n| n.id() == id).is_some_and(|n| n.is_locked()) {
                    return Err(forbid(format!("subtree contains locked node {id:?}")).node(id));
                }
                if doc.slots.iter().any(|s| &s.node_id == id) {
                    return Err(forbid(format!("node {id:?} is a slot target; remove the slot first")).node(id));
                }
            }
            for v in &mut doc.variants {
                v.node_patches.retain(|p| !subtree.contains(&p.node_id));
            }
            if let Node::Group { child_ids, .. } = node_mut(doc, &parent)? {
                child_ids.retain(|c| c != node_id);
            }
            doc.nodes.retain(|n| !subtree.iter().any(|s| s == n.id()));
        }
    }
    Ok(())
}
