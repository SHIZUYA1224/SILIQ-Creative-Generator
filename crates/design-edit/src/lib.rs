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
    Align, ApprovalStatus, Asset, Bounds, Code, DesignDocument, FocalPoint, Font, FontFeature, ImageFit, Issue, KinsokuPolicy, Language,
    Node, NodeStyle, OverflowPolicy, PackagePolicy, Paint, Slot, SlotTarget, SolidFill, SolidKind, TextOutline, TextRun, TextShadow,
    Variant, has_errors, validate,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overflow_policy: Option<OverflowPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_font_sizes: Option<Vec<f64>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Language>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kinsoku: Option<KinsokuPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<TextRun>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<FontFeature>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
pub enum Command {
    #[serde(rename_all = "camelCase")]
    GroupNodes {
        node_ids: Vec<String>,
        group_id: String,
        name: String,
    },
    #[serde(rename_all = "camelCase")]
    UngroupNodes {
        group_id: String,
    },
    ImportAsset {
        asset: Asset,
    },
    ImportFont {
        font: Font,
    },
    SetTitle {
        title: String,
    },
    #[serde(rename_all = "camelCase")]
    SetBrandSnapshot {
        brand_snapshot: design_model::BrandSnapshot,
    },
    SetArtboard {
        width: f64,
        height: f64,
    },
    #[serde(rename_all = "camelCase")]
    SetVariantApproval {
        variant_id: String,
        approval_status: ApprovalStatus,
    },
    SetVariant {
        variant: Variant,
    },
    #[serde(rename_all = "camelCase")]
    RemoveVariant {
        variant_id: String,
    },
    SetSlot {
        slot: Slot,
    },
    #[serde(rename_all = "camelCase")]
    RemoveSlot {
        slot_id: String,
    },
    #[serde(rename_all = "camelCase")]
    SetFontPolicy {
        font_id: String,
        package_policy: PackagePolicy,
    },
    #[serde(rename_all = "camelCase")]
    SetNodeStyle {
        node_id: String,
        style: NodeStyle,
    },
    #[serde(rename_all = "camelCase")]
    SetImageFit {
        node_id: String,
        fit: ImageFit,
        focal_point: FocalPoint,
        clip_to_bounds: bool,
    },
    #[serde(rename_all = "camelCase")]
    SetRectCornerRadius {
        node_id: String,
        corner_radius: f64,
    },
    #[serde(rename_all = "camelCase")]
    SetPathData {
        node_id: String,
        data: String,
    },
    #[serde(rename_all = "camelCase")]
    SetPathFillRule {
        node_id: String,
        fill_rule: design_model::PathRule,
    },
    #[serde(rename_all = "camelCase")]
    SetPathViewBox {
        node_id: String,
        view_box: Bounds,
    },
    #[serde(rename_all = "camelCase")]
    SetBounds {
        node_id: String,
        bounds: Bounds,
    },
    #[serde(rename_all = "camelCase")]
    SetRotation {
        node_id: String,
        rotation_deg: f64,
    },
    #[serde(rename_all = "camelCase")]
    SetOpacity {
        node_id: String,
        opacity: f64,
    },
    #[serde(rename_all = "camelCase")]
    SetVisible {
        node_id: String,
        visible: bool,
    },
    #[serde(rename_all = "camelCase")]
    SetLocked {
        node_id: String,
        locked: bool,
    },
    #[serde(rename_all = "camelCase")]
    SetText {
        node_id: String,
        value: String,
    },
    #[serde(rename_all = "camelCase")]
    SetTextStyle {
        node_id: String,
        style: TextStylePatch,
    },
    #[serde(rename_all = "camelCase")]
    SetTextEffects {
        node_id: String,
        outline: Option<TextOutline>,
        shadow: Option<TextShadow>,
    },
    #[serde(rename_all = "camelCase")]
    SetFill {
        node_id: String,
        fill: Paint,
    },
    #[serde(rename_all = "camelCase")]
    SetImageAsset {
        node_id: String,
        asset_id: String,
    },
    /// Move `node_id` to position `index` among its parent's children
    /// (0 = drawn first / bottom).
    #[serde(rename_all = "camelCase")]
    Reorder {
        node_id: String,
        index: usize,
    },
    #[serde(rename_all = "camelCase")]
    AddNode {
        parent_id: String,
        index: usize,
        node: Node,
    },
    /// Deletes the node and its whole subtree.
    #[serde(rename_all = "camelCase")]
    DeleteNode {
        node_id: String,
    },
    /// Several commands applied as one transaction (one Undo step).
    Batch {
        commands: Vec<Command>,
    },
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

    pub fn set_mode(&mut self, expected_revision: u64, mode: Mode) -> Result<u64, Vec<Issue>> {
        self.check_revision(expected_revision)?;
        let revision = self
            .doc
            .revision
            .checked_add(1)
            .ok_or_else(|| vec![forbid("revision limit reached")])?;
        self.mode = mode;
        self.doc.revision = revision;
        Ok(self.doc.revision)
    }

    pub fn can_undo(&self) -> bool {
        self.undo
            .last()
            .is_some_and(|previous| self.mode != Mode::Production || production_restore_allowed(&self.doc, previous))
    }

    pub fn can_redo(&self) -> bool {
        self.redo
            .last()
            .is_some_and(|next| self.mode != Mode::Production || production_restore_allowed(&self.doc, next))
    }

    /// Apply one command as one transaction. Returns the new revision.
    pub fn apply(&mut self, expected_revision: u64, cmd: &Command) -> Result<u64, Vec<Issue>> {
        self.check_revision(expected_revision)?;
        let mut next = self.doc.clone();
        if self.mode == Mode::Authoring && !approval_only(cmd) {
            for variant in &mut next.variants {
                variant.approval_status = ApprovalStatus::Draft;
            }
        }
        apply_command(&mut next, cmd, self.mode).map_err(|i| vec![i])?;
        let issues = validate(&next);
        if has_errors(&issues) {
            return Err(issues);
        }
        next.revision = self
            .doc
            .revision
            .checked_add(1)
            .ok_or_else(|| vec![forbid("revision limit reached")])?;
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
        if self.mode == Mode::Production
            && self
                .undo
                .last()
                .is_some_and(|previous| !production_restore_allowed(&self.doc, previous))
        {
            return Err(vec![forbid("production Undo cannot restore authoring changes to fixed properties")]);
        }
        let revision = self
            .doc
            .revision
            .checked_add(1)
            .ok_or_else(|| vec![forbid("revision limit reached")])?;
        let Some(mut prev) = self.undo.pop() else {
            return Err(vec![Issue::error(Code::EditForbidden, "nothing to undo", "")]);
        };
        prev.revision = revision;
        let cur = std::mem::replace(&mut self.doc, prev);
        self.redo.push(cur);
        Ok(self.doc.revision)
    }

    pub fn redo(&mut self, expected_revision: u64) -> Result<u64, Vec<Issue>> {
        self.check_revision(expected_revision)?;
        if self.mode == Mode::Production && self.redo.last().is_some_and(|next| !production_restore_allowed(&self.doc, next)) {
            return Err(vec![forbid("production Redo cannot restore authoring changes to fixed properties")]);
        }
        let revision = self
            .doc
            .revision
            .checked_add(1)
            .ok_or_else(|| vec![forbid("revision limit reached")])?;
        let Some(mut next) = self.redo.pop() else {
            return Err(vec![Issue::error(Code::EditForbidden, "nothing to redo", "")]);
        };
        next.revision = revision;
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
    // Global authoring commands use the same atomic validation and history.
    if apply_global(doc, cmd, mode)? {
        return Ok(());
    }
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
        Command::GroupNodes { node_ids, .. } => {
            let first = node_ids.first().ok_or_else(|| forbid("select at least two layers"))?;
            (parent_of(doc, first).ok_or_else(|| forbid("the root cannot be grouped"))?, false)
        }
        Command::UngroupNodes { group_id } => (group_id.as_str(), false),
        Command::AddNode { parent_id, .. } => (parent_id.as_str(), false),
        Command::SetNodeStyle { node_id, .. }
        | Command::SetImageFit { node_id, .. }
        | Command::SetRectCornerRadius { node_id, .. }
        | Command::SetPathData { node_id, .. }
        | Command::SetPathFillRule { node_id, .. }
        | Command::SetPathViewBox { node_id, .. }
        | Command::SetBounds { node_id, .. }
        | Command::SetRotation { node_id, .. }
        | Command::SetOpacity { node_id, .. }
        | Command::SetVisible { node_id, .. }
        | Command::SetLocked { node_id, .. }
        | Command::SetTextStyle { node_id, .. }
        | Command::SetTextEffects { node_id, .. }
        | Command::SetFill { node_id, .. }
        | Command::Reorder { node_id, .. }
        | Command::DeleteNode { node_id } => (node_id.as_str(), false),
        _ => unreachable!("global command handled above"),
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
        Command::SetNodeStyle { node_id, style } => {
            if style.node_id != *node_id {
                return Err(forbid("style nodeId must match the command target"));
            }
            if doc.nodes.iter().any(|n| n.id() == node_id && matches!(n, Node::Group { .. }))
                && doc
                    .node_styles
                    .iter()
                    .find(|s| s.node_id == *node_id)
                    .and_then(|s| s.affine)
                    .unwrap_or(IDENTITY)
                    != style.affine.unwrap_or(IDENTITY)
            {
                reject_locked_subtree(doc, node_id)?;
            }
            doc.schema_version = design_model::SCHEMA_VERSION.into();
            doc.node_styles.retain(|s| s.node_id != *node_id);
            doc.node_styles.push(style.clone());
        }
        Command::SetImageFit {
            node_id,
            fit: f,
            focal_point: fp,
            clip_to_bounds: clip,
        } => {
            if *f == ImageFit::Stretch || !clip {
                doc.schema_version = design_model::SCHEMA_VERSION.into();
            }
            match node_mut(doc, node_id)? {
                Node::Image {
                    fit,
                    focal_point,
                    clip_to_bounds,
                    ..
                } => {
                    *fit = *f;
                    *focal_point = *fp;
                    *clip_to_bounds = *clip;
                }
                _ => return Err(forbid("setImageFit applies to image nodes")),
            }
        }
        Command::SetRectCornerRadius { node_id, corner_radius: r } => match node_mut(doc, node_id)? {
            Node::Rect { corner_radius, .. } => *corner_radius = *r,
            _ => return Err(forbid("setRectCornerRadius applies to rect nodes")),
        },
        Command::SetPathFillRule { node_id, fill_rule: rule } => match node_mut(doc, node_id)? {
            Node::Path { fill_rule, .. } => *fill_rule = *rule,
            _ => return Err(forbid("setPathFillRule applies to path nodes")),
        },
        Command::SetPathViewBox { node_id, view_box: box_ } => match node_mut(doc, node_id)? {
            Node::Path { view_box, .. } => *view_box = *box_,
            _ => return Err(forbid("setPathViewBox applies to path nodes")),
        },
        Command::SetPathData { node_id, data: d } => match node_mut(doc, node_id)? {
            Node::Path { data, .. } => *data = d.clone(),
            _ => return Err(forbid("setPathData applies to path nodes")),
        },
        Command::Batch { .. } => unreachable!(),
        Command::GroupNodes { node_ids, group_id, name } => group_nodes(doc, node_ids, group_id, name)?,
        Command::UngroupNodes { group_id } => ungroup_nodes(doc, group_id)?,
        Command::SetBounds { node_id, bounds } => set_bounds(doc, node_id, *bounds)?,
        Command::SetRotation { node_id, rotation_deg } => {
            if doc.nodes.iter().any(|n| n.id() == node_id && matches!(n, Node::Group { .. })) {
                reject_locked_subtree(doc, node_id)?;
            }
            *node_mut(doc, node_id)?.common_mut().rotation_deg = *rotation_deg;
        }
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
        Command::SetTextStyle { node_id, style } => {
            if style.kinsoku.is_some() || style.runs.is_some() || style.features.is_some() {
                doc.schema_version = design_model::SCHEMA_VERSION.into();
            }
            match node_mut(doc, node_id)? {
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
                    if let Some(v) = style.overflow_policy {
                        text.overflow_policy = v;
                    }
                    if let Some(v) = &style.allowed_font_sizes {
                        text.allowed_font_sizes = v.clone();
                    }
                    if let Some(v) = style.language {
                        text.language = v;
                    }
                    if let Some(v) = style.kinsoku {
                        text.kinsoku = v;
                    }
                    if let Some(v) = &style.runs {
                        text.runs = v.clone();
                    }
                    if let Some(v) = &style.features {
                        text.features = v.clone();
                    }
                    if let Some(v) = &style.color {
                        *fill = SolidFill {
                            kind: SolidKind::Solid,
                            color: v.clone(),
                        };
                    }
                }
                _ => return Err(forbid("setTextStyle applies to text nodes").node(node_id)),
            }
        }
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
        Command::SetFill { node_id, fill: f } => {
            if matches!(f, Paint::RadialGradient { .. }) {
                doc.schema_version = design_model::SCHEMA_VERSION.into();
            }
            match node_mut(doc, node_id)? {
                Node::Rect { fill, .. } | Node::Path { fill, .. } => *fill = f.clone(),
                _ => return Err(forbid("setFill applies to rect/path nodes").node(node_id)),
            }
        }
        Command::SetImageAsset { node_id, asset_id: a } => match node_mut(doc, node_id)? {
            Node::Image { asset_id, .. } | Node::Logo { asset_id, .. } => *asset_id = a.clone(),
            _ => return Err(forbid("setImageAsset applies to image/logo nodes").node(node_id)),
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
            if matches!(node, Node::Path { .. }) {
                doc.schema_version = design_model::SCHEMA_VERSION.into();
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
            doc.node_styles.retain(|s| !subtree.contains(&s.node_id));
        }
        _ => unreachable!("global command handled above"),
    }
    Ok(())
}

fn approval_only(cmd: &Command) -> bool {
    match cmd {
        Command::SetVariantApproval { .. } => true,
        Command::Batch { commands } => !commands.is_empty() && commands.iter().all(approval_only),
        _ => false,
    }
}

fn apply_global(doc: &mut DesignDocument, cmd: &Command, mode: Mode) -> Result<bool, Issue> {
    let global = matches!(
        cmd,
        Command::ImportAsset { .. }
            | Command::ImportFont { .. }
            | Command::SetTitle { .. }
            | Command::SetBrandSnapshot { .. }
            | Command::SetArtboard { .. }
            | Command::SetVariantApproval { .. }
            | Command::SetVariant { .. }
            | Command::RemoveVariant { .. }
            | Command::SetSlot { .. }
            | Command::RemoveSlot { .. }
            | Command::SetFontPolicy { .. }
    );
    if !global {
        return Ok(false);
    }
    if mode != Mode::Authoring {
        return Err(forbid("global document changes require authoring mode"));
    }
    match cmd {
        Command::ImportAsset { asset } => {
            if let Some(existing) = doc.assets.iter_mut().find(|a| a.id == asset.id) {
                *existing = asset.clone();
            } else {
                doc.assets.push(asset.clone());
            }
        }
        Command::ImportFont { font } => {
            if let Some(existing) = doc.fonts.iter_mut().find(|f| f.id == font.id) {
                *existing = font.clone();
            } else {
                doc.fonts.push(font.clone());
            }
        }
        Command::SetTitle { title } => doc.title = title.clone(),
        Command::SetBrandSnapshot { brand_snapshot } => {
            doc.schema_version = design_model::SCHEMA_VERSION.into();
            doc.brand_snapshot = Some(brand_snapshot.clone());
        }
        Command::SetArtboard { width, height } => {
            doc.base_artboard.width = *width;
            doc.base_artboard.height = *height;
        }
        Command::SetVariantApproval {
            variant_id,
            approval_status,
        } => {
            let var = doc
                .variants
                .iter_mut()
                .find(|v| v.id == *variant_id)
                .ok_or_else(|| forbid("variant does not exist"))?;
            var.approval_status = *approval_status;
        }
        Command::SetVariant { variant } => {
            // Changing an existing size's group transform must obey the same
            // descendant locks as the base-artboard transform commands.
            if let Some(previous) = doc.variants.iter().find(|v| v.id == variant.id) {
                for patch in &variant.node_patches {
                    let old = previous.node_patches.iter().find(|p| p.node_id == patch.node_id);
                    if old.is_none_or(|p| p.bounds != patch.bounds || p.affine != patch.affine)
                        && doc.nodes.iter().any(|n| n.id() == patch.node_id && matches!(n, Node::Group { .. }))
                    {
                        reject_locked_subtree(doc, &patch.node_id)?;
                    }
                }
                for old in &previous.node_patches {
                    if !variant.node_patches.iter().any(|p| p.node_id == old.node_id)
                        && doc.nodes.iter().any(|n| n.id() == old.node_id && matches!(n, Node::Group { .. }))
                    {
                        reject_locked_subtree(doc, &old.node_id)?;
                    }
                }
            }
            if variant.node_patches.iter().any(|p| p.affine.is_some()) {
                doc.schema_version = design_model::SCHEMA_VERSION.into();
            }
            let mut draft = variant.clone();
            draft.approval_status = ApprovalStatus::Draft;
            if let Some(previous) = doc.variants.iter_mut().find(|v| v.id == variant.id) {
                *previous = draft;
            } else {
                doc.variants.push(draft);
            }
        }
        Command::RemoveVariant { variant_id } => {
            if !doc.variants.iter().any(|v| v.id == *variant_id) {
                return Err(forbid("variant does not exist"));
            }
            doc.variants.retain(|v| v.id != *variant_id);
        }
        Command::SetSlot { slot } => {
            doc.slots.retain(|s| s.id != slot.id);
            doc.slots.push(slot.clone());
        }
        Command::RemoveSlot { slot_id } => doc.slots.retain(|s| s.id != *slot_id),
        Command::SetFontPolicy { font_id, package_policy } => {
            let font = doc
                .fonts
                .iter_mut()
                .find(|f| f.id == *font_id)
                .ok_or_else(|| forbid("font does not exist"))?;
            font.package_policy = *package_policy;
        }
        _ => unreachable!(),
    }
    Ok(true)
}

/// Production history may restore only the same declared slot properties.
/// History is retained while changing modes, without granting authoring rights.
fn production_restore_allowed(current: &DesignDocument, target: &DesignDocument) -> bool {
    let mut allowed = current.clone();
    allowed.revision = target.revision;
    for node in &mut allowed.nodes {
        let id = node.id().to_owned();
        let Some(next) = target.nodes.iter().find(|n| n.id() == id) else {
            return false;
        };
        for slot in current.slots.iter().filter(|s| s.node_id == id) {
            match (slot.target, &mut *node, next) {
                (SlotTarget::TextValue, Node::Text { text, .. }, Node::Text { text: next_text, .. }) => {
                    text.value = next_text.value.clone()
                }
                (SlotTarget::AssetId, Node::Image { asset_id, .. }, Node::Image { asset_id: next_asset, .. }) => {
                    *asset_id = next_asset.clone()
                }
                _ => return false,
            }
        }
    }
    allowed == *target
}

const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
fn multiply_affine(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]
}
fn subtree_ids(doc: &DesignDocument, id: &str) -> Vec<String> {
    let mut ids = vec![id.to_owned()];
    let mut index = 0;
    while index < ids.len() {
        if let Some(Node::Group { child_ids, .. }) = doc.nodes.iter().find(|n| n.id() == ids[index]) {
            ids.extend(child_ids.iter().cloned());
        }
        index += 1;
    }
    ids
}
fn reject_locked_subtree(doc: &DesignDocument, id: &str) -> Result<(), Issue> {
    for child in subtree_ids(doc, id) {
        if effectively_locked(doc, &child) {
            return Err(forbid("a locked descendant prevents transforming this group").node(&child));
        }
    }
    Ok(())
}
fn set_bounds(doc: &mut DesignDocument, id: &str, bounds: Bounds) -> Result<(), Issue> {
    let is_group = doc.nodes.iter().any(|n| n.id() == id && matches!(n, Node::Group { .. }));
    if is_group {
        reject_locked_subtree(doc, id)?;
        let old = *doc.nodes.iter().find(|n| n.id() == id).unwrap().common().0;
        if !bounds.width.is_finite() || !bounds.height.is_finite() || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Err(forbid("group dimensions must be positive finite values"));
        }
        let sx = bounds.width / old.width;
        let sy = bounds.height / old.height;
        let resize = [sx, 0.0, 0.0, sy, bounds.x - sx * old.x, bounds.y - sy * old.y];
        let index = doc.node_styles.iter().position(|s| s.node_id == id).unwrap_or_else(|| {
            doc.node_styles.push(NodeStyle::new(id));
            doc.node_styles.len() - 1
        });
        let previous = doc.node_styles[index].affine.unwrap_or(IDENTITY);
        doc.node_styles[index].affine = Some(multiply_affine(resize, previous));
        doc.schema_version = design_model::SCHEMA_VERSION.into();
    }
    *node_mut(doc, id)?.common_mut().bounds = bounds;
    Ok(())
}
fn group_nodes(doc: &mut DesignDocument, ids: &[String], group_id: &str, name: &str) -> Result<(), Issue> {
    if ids.len() < 2 || ids.len() > 2000 {
        return Err(forbid("grouping needs 2..=2000 layers"));
    }
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    if unique.len() != ids.len() {
        return Err(forbid("group layer IDs must be unique"));
    }
    if doc.nodes.iter().any(|n| n.id() == group_id) {
        return Err(forbid("group ID already exists"));
    }
    let parent = parent_of(doc, &ids[0])
        .ok_or_else(|| forbid("the root cannot be grouped"))?
        .to_owned();
    if ids.iter().any(|id| parent_of(doc, id) != Some(parent.as_str())) {
        return Err(forbid("grouped layers must share the same parent"));
    }
    for id in ids {
        reject_locked_subtree(doc, id)?;
    }
    let children = match doc.nodes.iter().find(|n| n.id() == parent).unwrap() {
        Node::Group { child_ids, .. } => child_ids.clone(),
        _ => unreachable!(),
    };
    let positions: Vec<_> = children
        .iter()
        .enumerate()
        .filter(|(_, id)| ids.contains(id))
        .map(|(i, _)| i)
        .collect();
    if positions.last().unwrap() - positions[0] + 1 != ids.len() {
        return Err(forbid("group adjacent layers to preserve the exact paint order"));
    }
    let ordered: Vec<_> = positions.iter().map(|i| children[*i].clone()).collect();
    let mut left = f64::INFINITY;
    let mut top = f64::INFINITY;
    let mut right = f64::NEG_INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    for id in &ordered {
        let node = doc.nodes.iter().find(|n| n.id() == id).unwrap();
        let (b, rotation, _, _) = node.common();
        let (sin, cos) = rotation.to_radians().sin_cos();
        let (cx, cy) = (b.x + b.width / 2.0, b.y + b.height / 2.0);
        let rotate = [cos, sin, -sin, cos, cx - cos * cx + sin * cy, cy - sin * cx - cos * cy];
        let affine = doc
            .node_styles
            .iter()
            .find(|s| s.node_id == *id)
            .and_then(|s| s.affine)
            .unwrap_or(IDENTITY);
        let m = if matches!(node, Node::Group { .. }) {
            rotate
        } else {
            multiply_affine(rotate, affine)
        };
        for (x, y) in [
            (b.x, b.y),
            (b.x + b.width, b.y),
            (b.x, b.y + b.height),
            (b.x + b.width, b.y + b.height),
        ] {
            let px = m[0] * x + m[2] * y + m[4];
            let py = m[1] * x + m[3] * y + m[5];
            left = left.min(px);
            right = right.max(px);
            top = top.min(py);
            bottom = bottom.max(py);
        }
    }
    let group = Node::Group {
        id: group_id.into(),
        name: name.into(),
        bounds: Bounds {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        },
        rotation_deg: 0.0,
        opacity: 1.0,
        visible: true,
        locked: false,
        child_ids: ordered,
    };
    if let Node::Group { child_ids, .. } = node_mut(doc, &parent)? {
        child_ids.splice(positions[0]..positions[0] + ids.len(), [group_id.to_owned()]);
    }
    doc.nodes.push(group);
    Ok(())
}
fn ungroup_nodes(doc: &mut DesignDocument, id: &str) -> Result<(), Issue> {
    let parent = parent_of(doc, id).ok_or_else(|| forbid("the root cannot be ungrouped"))?.to_owned();
    reject_locked_subtree(doc, id)?;
    let children = match doc.nodes.iter().find(|n| n.id() == id) {
        Some(Node::Group {
            child_ids,
            rotation_deg,
            opacity,
            visible,
            ..
        }) if *rotation_deg == 0.0 && *opacity == 1.0 && *visible => child_ids.clone(),
        Some(Node::Group { .. }) => {
            return Err(forbid(
                "ungrouping a hidden, rotated or translucent group would change its appearance",
            ));
        }
        _ => return Err(forbid("choose a group to ungroup")),
    };
    if doc.node_styles.iter().find(|s| s.node_id == id).is_some_and(|s| {
        s.affine.is_some_and(|m| m != IDENTITY)
            || s.blend_mode != design_model::BlendMode::Normal
            || !s.effects.is_empty()
            || s.clip.is_some()
            || s.mask.is_some()
    }) {
        return Err(forbid(
            "ungrouping a transformed/effected/clipped group requires an explicitly approved flatten operation",
        ));
    }
    if doc.node_styles.iter().any(|s| s.protected_from.iter().any(|target| target == id)) {
        return Err(forbid("this group is a logo protection target; revise that rule before ungrouping"));
    }
    if let Node::Group { child_ids, .. } = node_mut(doc, &parent)? {
        let index = child_ids.iter().position(|child| child == id).unwrap();
        child_ids.splice(index..index + 1, children);
    }
    doc.nodes.retain(|n| n.id() != id);
    doc.node_styles.retain(|s| s.node_id != id);
    for variant in &mut doc.variants {
        variant.node_patches.retain(|p| p.node_id != id);
    }
    Ok(())
}
