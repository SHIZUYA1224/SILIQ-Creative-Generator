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
//! Not implemented yet: cancel / resume, partial-output mode, parallelism.

pub mod csv;
pub mod naming;

use design_model::{Code, DesignDocument, Issue, Issues, bind_row, load_document, require_approved, resolve_variant};
use design_render::resources::{ResourceStore, sha256_hex};
use design_render::{RENDER_PROFILE, RenderOptions, RenderProfile, render_document, text::FontBackend};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Serialize)]
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
}

struct Planned {
    row_id: String,
    values: BTreeMap<String, String>,
    variant_id: String,
    file: String,
}

pub fn run_batch(req: &BatchRequest) -> Result<Manifest, Issues> {
    let doc = load_document(req.template)?;
    let table = csv::parse(req.data).map_err(|i| vec![i])?;
    let planned = plan(&doc, &table, req)?;

    // ---- render into staging
    let staging = create_staging(req.out_dir).map_err(|i| vec![i])?;
    let result = render_all(&doc, &planned, req, &staging);
    let items = match result {
        Ok(items) => items,
        Err(issues) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(issues);
        }
    };

    // ---- publish
    let mut published: Vec<PathBuf> = Vec::new();
    for item in &items {
        let target = req.out_dir.join(&item.file);
        if let Err(e) = fs::hard_link(staging.join(&item.file), &target) {
            for p in &published {
                let _ = fs::remove_file(p);
            }
            let _ = fs::remove_dir_all(&staging);
            return Err(vec![publish_error(&item.file, e)]);
        }
        published.push(target);
    }
    let _ = fs::remove_dir_all(&staging);

    let manifest = Manifest {
        status: "completed",
        error_policy: "block-all",
        template_document_id: doc.document_id.clone(),
        template_revision: doc.revision,
        template_sha256: sha256_hex(req.template),
        data_sha256: sha256_hex(req.data),
        rows: table.records.len(),
        variants: req.variants.to_vec(),
        total: items.len(),
        render_profile: RENDER_PROFILE,
        font_backend: req.options.font_backend,
        items,
    };
    let json = serde_json::to_vec_pretty(&manifest).expect("serializable");
    write_new(&req.out_dir.join(req.manifest_name), &json).map_err(|i| vec![i])?;
    Ok(manifest)
}

fn plan(doc: &DesignDocument, table: &csv::Table, req: &BatchRequest) -> Result<Vec<Planned>, Issues> {
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
    names.insert(req.manifest_name.to_owned());
    if let Err(i) = naming::validate_output_name(req.manifest_name, "json") {
        issues.push(i);
    }
    if req.out_dir.join(req.manifest_name).symlink_metadata().is_ok() {
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
            if !names.insert(file.clone()) {
                issues.push(Issue::error(
                    Code::OutputCollision,
                    format!("two outputs would be named {file}"),
                    "Include {rowId} and {variantId} in the name pattern.",
                ));
                continue;
            }
            if req.out_dir.join(&file).symlink_metadata().is_ok() {
                issues.push(collision(&file).row(row_id).variant(v));
                continue;
            }
            planned.push(Planned {
                row_id: row_id.clone(),
                values: values.clone(),
                variant_id: v.clone(),
                file,
            });
        }
    }
    if issues.is_empty() { Ok(planned) } else { Err(issues) }
}

fn render_all(doc: &DesignDocument, planned: &[Planned], req: &BatchRequest, staging: &Path) -> Result<Vec<ManifestItem>, Issues> {
    let template_sha = sha256_hex(req.template);
    let mut issues = Vec::new();
    let mut items = Vec::new();
    for p in planned {
        let tag = |mut i: Issue| {
            i.location.row_id.get_or_insert_with(|| p.row_id.clone());
            i.location.variant_id.get_or_insert_with(|| p.variant_id.clone());
            i
        };
        let bound = match bind_row(doc, &p.row_id, &p.values) {
            Ok(d) => d,
            Err(e) => {
                issues.extend(e.into_iter().map(tag));
                continue;
            }
        };
        match render_document(&bound, &p.variant_id, req.store, req.options) {
            Ok(out) => {
                if let Err(i) = write_new(&staging.join(&p.file), &out.png) {
                    issues.push(tag(i));
                    continue;
                }
                let digest_input = serde_json::to_vec(&(&template_sha, &p.variant_id, &p.values)).expect("serializable");
                items.push(ManifestItem {
                    row_id: p.row_id.clone(),
                    variant_id: p.variant_id.clone(),
                    file: p.file.clone(),
                    sha256: out.report.output_sha256.clone(),
                    width: out.report.width,
                    height: out.report.height,
                    input_digest: sha256_hex(&digest_input),
                    applied_font_sizes: out.report.text.iter().map(|t| (t.node_id.clone(), t.applied_font_size)).collect(),
                    warnings: out.report.warnings.into_iter().map(tag).collect(),
                });
            }
            Err(e) => issues.extend(e.into_iter().map(tag)),
        }
    }
    if issues.is_empty() { Ok(items) } else { Err(issues) }
}

fn create_staging(out_dir: &Path) -> Result<PathBuf, Issue> {
    let dir = out_dir.join(format!(".siliq-batch-{}", std::process::id()));
    fs::create_dir(&dir).map_err(|e| {
        Issue::error(
            Code::PathDenied,
            format!("cannot create staging directory: {}", e.kind()),
            "Check that the output directory is writable.",
        )
    })?;
    Ok(dir)
}

fn collision(name: &str) -> Issue {
    Issue::error(
        Code::OutputCollision,
        format!("{name} already exists in the output directory"),
        "Choose a new output directory; existing files are never overwritten.",
    )
}

fn publish_error(name: &str, e: std::io::Error) -> Issue {
    if e.kind() == std::io::ErrorKind::AlreadyExists {
        collision(name)
    } else {
        Issue::error(
            Code::PathDenied,
            format!("cannot publish {name}: {}", e.kind()),
            "Check the output directory.",
        )
    }
}

/// Create `path` (must not exist), write, flush.
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Issue> {
    let fail = |e: std::io::Error| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            collision(&path.file_name().unwrap_or_default().to_string_lossy())
        } else {
            Issue::error(
                Code::PathDenied,
                format!("cannot write output: {}", e.kind()),
                "Check that the output directory is writable and has free space.",
            )
        }
    };
    let mut f = fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(fail)?;
    f.write_all(bytes).map_err(fail)?;
    f.sync_all().map_err(fail)?;
    Ok(())
}
