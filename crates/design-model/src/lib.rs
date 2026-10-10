//! SILIQ Creative Generator — core document model (Phase 0 subset).
//!
//! Mirrors `schemas/design.core.schema.json` and adds the semantic checks the
//! schema cannot express. No I/O, no rendering.

pub mod issue;
pub mod types;
pub mod validate;
pub mod variant;

pub use issue::{Code, Issue, Issues, Location, Severity, has_errors};
pub use types::*;
pub use validate::{parse_document, validate};
pub use variant::{ResolvedVariant, require_approved, resolve_variant};

/// Parse and validate in one step.
pub fn load_document(bytes: &[u8]) -> Result<DesignDocument, Issues> {
    let doc = parse_document(bytes).map_err(|i| vec![i])?;
    let issues = validate(&doc);
    if has_errors(&issues) { Err(issues) } else { Ok(doc) }
}
