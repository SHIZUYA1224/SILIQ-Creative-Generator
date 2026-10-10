//! Slot binding (TPL-001, DES-001): apply one data row to a template.
//!
//! Only the property a slot declares is written (`text.value` or `assetId`).
//! Values are used exactly as given — no trimming, normalisation, number
//! formatting or formula evaluation.

use crate::issue::{Code, Issue, Issues};
use crate::types::*;
use std::collections::BTreeMap;

/// Bind `values` (slot id → raw value) for the row `row_id`.
///
/// - Every key must be a declared slot.
/// - A required slot must be present and non-empty.
/// - An optional slot that is absent or empty keeps the template's value.
/// - Text values are limited by the slot's `maxLength` (in characters).
/// - Image values must name a declared raster asset.
pub fn bind_row(doc: &DesignDocument, row_id: &str, values: &BTreeMap<String, String>) -> Result<DesignDocument, Issues> {
    let mut issues = Vec::new();
    let mut out = doc.clone();

    for key in values.keys() {
        if !doc.slots.iter().any(|s| &s.id == key) {
            issues.push(
                Issue::error(
                    Code::DataInvalid,
                    format!("column {key:?} is not a slot of this template"),
                    "Rename the column to a slot id, or remove it.",
                )
                .row(row_id),
            );
        }
    }

    for slot in &doc.slots {
        let value = values.get(&slot.id).filter(|v| !v.is_empty());
        let Some(value) = value else {
            if slot.required {
                issues.push(
                    Issue::error(
                        Code::DataInvalid,
                        format!("required slot {:?} is empty", slot.id),
                        "Fill in the value; required slots are never generated or left blank.",
                    )
                    .row(row_id)
                    .node(&slot.node_id),
                );
            }
            continue;
        };
        let node = out
            .nodes
            .iter_mut()
            .find(|n| n.id() == slot.node_id)
            .expect("validated slot target");
        match (slot.target, node) {
            (SlotTarget::TextValue, Node::Text { text, .. }) => {
                let len = value.chars().count();
                if let Some(max) = slot.max_length
                    && len > max as usize
                {
                    issues.push(
                        Issue::error(
                            Code::DataInvalid,
                            format!("slot {:?} has {len} characters; the template allows {max}", slot.id),
                            "Shorten the value.",
                        )
                        .row(row_id)
                        .node(&slot.node_id),
                    );
                    continue;
                }
                text.value = value.clone();
            }
            (SlotTarget::AssetId, Node::Image { asset_id, .. }) => {
                let ok = doc.assets.iter().any(|a| &a.id == value && a.kind == AssetKind::Raster);
                if !ok {
                    issues.push(
                        Issue::error(
                            Code::AssetMissing,
                            format!("slot {:?}: {value:?} is not a declared raster asset", slot.id),
                            "Use the id of an image registered in the template.",
                        )
                        .row(row_id)
                        .node(&slot.node_id),
                    );
                    continue;
                }
                *asset_id = value.clone();
            }
            _ => unreachable!("validate() guarantees slot/node typing"),
        }
    }

    if issues.is_empty() { Ok(out) } else { Err(issues) }
}
