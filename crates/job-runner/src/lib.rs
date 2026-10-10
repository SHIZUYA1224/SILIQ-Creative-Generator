//! Batch production (Phase 3 minimal slice): template × CSV rows × variants
//! → PNG files + manifest.
//!
//! 1. Plan: validate the template, parse the CSV, check variants, expand and
//!    check every output name (no duplicates, nothing already present).
//! 2. Render every item into a private staging directory inside the output
//!    directory, collecting every issue (row / variant / node).
//! 3. If any item failed, delete the staging directory: **nothing is
//!    published** (error policy `block-all`).
//! 4. Otherwise publish each file with `hard_link` (never overwrites), then
//!    write the manifest last.
//!
//! The resource store caches verified bytes, so all items see the same
//! fonts and assets even if files change on disk during the job (BAT-003).
//! Controlled jobs add progress, cancellation and checksum-verified resume.

mod controlled;
pub mod csv;
mod io;
pub mod naming;
pub use controlled::{
    BatchPreflight, BatchProgress, ErrorPolicy, PlannedItem, batch_input_digest, export_batch_input_digest, preflight,
    preflight_export_granted, preflight_granted, preflight_resume_granted, resume_batch_controlled, run_batch_controlled,
    run_batch_granted, run_export_batch_granted,
};

use design_model::{Code, DesignDocument, Issue, Issues, require_approved, resolve_variant};
use design_render::resources::ResourceStore;
use design_render::{RenderOptions, RenderProfile, text::FontBackend};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::Path;

pub struct BatchRequest<'a> {
    pub template: &'a [u8],
    pub data: &'a [u8],
    pub variants: &'a [String],
    pub name_pattern: &'a str,
    pub manifest_name: &'a str,
    pub store: &'a ResourceStore,
    /// Canonical, existing, non-symlink directory (the user's write grant).
    pub out_dir: &'a Path,
    pub options: RenderOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestItem {
    pub row_id: String,
    pub variant_id: String,
    pub file: String,
    pub sha256: String,
    pub width: u32,
    pub height: u32,
    /// SHA-256 over the template bytes, variant id and the row's values.
    pub input_digest: String,
    /// Applied size per text node (shows any approved fit reduction).
    pub applied_font_sizes: BTreeMap<String, f64>,
    pub warnings: Vec<Issue>,
    pub format: design_export::ExportFormat,
    pub export_report: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub status: &'static str,
    pub error_policy: &'static str,
    pub template_document_id: String,
    pub template_revision: u64,
    pub template_sha256: String,
    pub data_sha256: String,
    pub rows: usize,
    pub variants: Vec<String>,
    pub total: usize,
    pub render_profile: RenderProfile,
    pub font_backend: FontBackend,
    pub items: Vec<ManifestItem>,
    pub input_digest: String,
    pub issues: Vec<Issue>,
    pub export_recipes: Vec<design_export::ExportRecipe>,
}

#[derive(Clone)]
struct Planned {
    row_id: String,
    values: BTreeMap<String, String>,
    variant_id: String,
    file: String,
    recipe: design_export::ExportRecipe,
}

pub fn run_batch(req: &BatchRequest) -> Result<Manifest, Issues> {
    run_batch_controlled(req, &std::sync::atomic::AtomicBool::new(false), |_| {})
}

fn plan(
    doc: &DesignDocument,
    table: &csv::Table,
    req: &BatchRequest,
    directory: &std::fs::File,
    resume: bool,
) -> Result<Vec<Planned>, Issues> {
    let mut issues = Vec::new();

    // Columns: rowId plus slot ids; every required slot must have a column.
    let Some(id_col) = table.header.iter().position(|h| h == csv::ROW_ID_COLUMN) else {
        return Err(vec![Issue::error(
            Code::DataInvalid,
            "CSV has no rowId column",
            "Add a rowId column with a unique id per row.",
        )]);
    };
    for h in table.header.iter().filter(|h| *h != csv::ROW_ID_COLUMN) {
        if !doc.slots.iter().any(|s| &s.id == h) {
            issues.push(Issue::error(
                Code::DataInvalid,
                format!("column {h:?} is not a slot of the template"),
                format!(
                    "Use one of: {}",
                    doc.slots.iter().map(|s| s.id.as_str()).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
    }
    for s in doc.slots.iter().filter(|s| s.required) {
        if !table.header.contains(&s.id) {
            issues.push(Issue::error(
                Code::DataInvalid,
                format!("required slot {:?} has no column", s.id),
                "Add the column.",
            ));
        }
    }
    if table.records.is_empty() {
        issues.push(Issue::error(Code::DataInvalid, "CSV has no data rows", "Add at least one row."));
    }

    // Variants: must exist and be approved.
    if req.variants.is_empty() {
        issues.push(Issue::error(
            Code::LayoutUndefined,
            "no variant requested",
            "Name at least one variant.",
        ));
    }
    let mut seen_variants = HashSet::new();
    for v in req.variants {
        if !seen_variants.insert(v.as_str()) {
            issues.push(
                Issue::error(
                    Code::LayoutUndefined,
                    format!("variant {v:?} requested twice"),
                    "List each variant once.",
                )
                .variant(v),
            );
            continue;
        }
        match resolve_variant(doc, v) {
            Ok(rv) => {
                if let Err(i) = require_approved(&rv) {
                    issues.push(i);
                }
            }
            Err(i) => issues.push(i),
        }
    }

    // Rows and names.
    let mut row_ids = HashSet::new();
    let mut names = HashSet::new();
    names.insert(req.manifest_name.to_ascii_lowercase());
    if let Err(i) = naming::validate_output_name(req.manifest_name, "json") {
        issues.push(i);
    }
    if !resume && io::exists(directory, req.manifest_name).map_err(|i| vec![i])? {
        issues.push(collision(req.manifest_name));
    }
    let mut planned = Vec::new();
    for (line, rec) in &table.records {
        let row_id = &rec[id_col];
        if !csv::is_valid_row_id(row_id) {
            issues.push(Issue::error(
                Code::DataInvalid,
                format!("CSV line {line}: rowId {row_id:?} must be 1–64 of A-Z a-z 0-9 _ - (starting with a letter or digit)"),
                "Fix the rowId.",
            ));
            continue;
        }
        if !row_ids.insert(row_id.clone()) {
            issues.push(
                Issue::error(
                    Code::DataInvalid,
                    format!("CSV line {line}: duplicate rowId"),
                    "Make every rowId unique.",
                )
                .row(row_id),
            );
            continue;
        }
        let values: BTreeMap<String, String> = table
            .header
            .iter()
            .zip(rec)
            .filter(|(h, _)| *h != csv::ROW_ID_COLUMN)
            .map(|(h, v)| (h.clone(), v.clone()))
            .collect();
        for v in req.variants {
            let file = match naming::expand_pattern(req.name_pattern, row_id, v) {
                Ok(f) => f,
                Err(i) => return Err(vec![i]),
            };
            if let Err(i) = naming::validate_output_name(&file, "png") {
                issues.push(i.row(row_id).variant(v));
                continue;
            }
            if !names.insert(file.to_ascii_lowercase()) {
                issues.push(Issue::error(
                    Code::OutputCollision,
                    format!("two outputs would be named {file}"),
                    "Include {rowId} and {variantId} in the name pattern.",
                ));
                continue;
            }
            if !resume && io::exists(directory, &file).map_err(|i| vec![i])? {
                issues.push(collision(&file).row(row_id).variant(v));
                continue;
            }
            planned.push(Planned {
                row_id: row_id.clone(),
                values: values.clone(),
                variant_id: v.clone(),
                file,
                recipe: design_export::ExportRecipe::default(),
            });
        }
    }
    if planned.len() > 20_000 {
        issues.push(Issue::error(
            Code::ResourceLimit,
            "more than 20,000 batch items",
            "Split the dataset or select fewer variants.",
        ));
    }
    if issues.is_empty() { Ok(planned) } else { Err(issues) }
}

fn collision(name: &str) -> Issue {
    Issue::error(
        Code::OutputCollision,
        format!("{name} already exists in the output directory"),
        "Choose a new output directory; existing files are never overwritten.",
    )
}

static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
fn next_nonce() -> u64 {
    NONCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// Create `path` (must not exist), write, flush.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Issue> {
    let parent = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let directory = io::directory(parent)?;
    let name = path.file_name().and_then(|s| s.to_str()).ok_or_else(io::denied)?;
    io::atomic_json(&directory, name, bytes, false)
}
