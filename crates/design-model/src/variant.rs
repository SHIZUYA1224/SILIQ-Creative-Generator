//! Variant resolution: apply the author's explicit node patches. Nothing is
//! inferred — unknown variants are `E_LAYOUT_UNDEFINED`, and draft variants are
//! refused for export (ADR-008).

use crate::issue::{Code, Issue};
use crate::types::*;

#[derive(Debug, Clone)]
pub struct ResolvedVariant {
    pub variant_id: String,
    pub width: f64,
    pub height: f64,
    pub approval_status: ApprovalStatus,
    /// Nodes with this variant's patches applied, in document order.
    pub nodes: Vec<Node>,
    pub node_styles: Vec<NodeStyle>,
}

/// Resolve `variant_id` against a document that already passed `validate`.
pub fn resolve_variant(doc: &DesignDocument, variant_id: &str) -> Result<ResolvedVariant, Issue> {
    let var = doc.variants.iter().find(|v| v.id == variant_id).ok_or_else(|| {
        Issue::error(
            Code::LayoutUndefined,
            format!("variant {variant_id:?} is not defined in this document"),
            "Choose one of the variants the author defined; layouts are never generated automatically.",
        )
        .variant(variant_id)
    })?;

    let mut nodes = doc.nodes.clone();
    let mut node_styles = doc.node_styles.clone();
    for p in &var.node_patches {
        // validate() guarantees the node exists and that fontSize targets text.
        let node = nodes.iter_mut().find(|n| n.id() == p.node_id).expect("validated patch target");
        *node.bounds_mut() = p.bounds;
        if let Some(affine) = p.affine {
            let index = node_styles.iter().position(|s| s.node_id == p.node_id).unwrap_or_else(|| {
                node_styles.push(NodeStyle::new(&p.node_id));
                node_styles.len() - 1
            });
            node_styles[index].affine = Some(affine);
        }
        if let Node::Text { text, .. } = &mut *node {
            if let Some(fs) = p.font_size {
                text.font_size = fs;
            }
            if let Some(lh) = p.line_height {
                text.line_height = lh;
            }
        }
    }

    Ok(ResolvedVariant {
        variant_id: var.id.clone(),
        width: var.width,
        height: var.height,
        approval_status: var.approval_status,
        nodes,
        node_styles,
    })
}

/// Production export only accepts approved variants.
pub fn require_approved(v: &ResolvedVariant) -> Result<(), Issue> {
    if v.approval_status == ApprovalStatus::Approved {
        Ok(())
    } else {
        Err(Issue::error(
            Code::VariantNotApproved,
            format!("variant {:?} is a draft", v.variant_id),
            "Have the author approve the variant before exporting it.",
        )
        .variant(&v.variant_id))
    }
}
