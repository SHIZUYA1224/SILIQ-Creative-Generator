use super::*;
use design_export::{ExportFormat, ExportRecipe, ExportReport};
use design_model::{bind_row, has_errors, load_document};
use design_render::resources::sha256_hex;
use design_render::{RENDER_PROFILE, RenderReport, render_document};
use serde_json::Value;
use std::fs::File;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_JOURNAL_BYTES: u64 = 20 * 1024 * 1024;
const MAX_PNG_BYTES: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ErrorPolicy {
    BlockAll,
    ValidOnly,
}
impl ErrorPolicy {
    fn name(self) -> &'static str {
        match self {
            Self::BlockAll => "block-all",
            Self::ValidOnly => "valid-only",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedItem {
    pub row_id: String,
    pub variant_id: String,
    pub file: String,
    pub issues: Issues,
    pub format: ExportFormat,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchPreflight {
    pub rows: usize,
    pub total: usize,
    pub variant_ids: Vec<String>,
    pub items: Vec<PlannedItem>,
    pub issues: Issues,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgress {
    pub status: String,
    pub total: usize,
    pub completed: usize,
    pub current_row_id: Option<String>,
    pub current_variant_id: Option<String>,
    pub issues: Issues,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Journal {
    version: u32,
    input_digest: String,
    error_policy: ErrorPolicy,
    status: String,
    items: Vec<ManifestItem>,
    published: Vec<String>,
    issues: Issues,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedManifest {
    status: String,
    error_policy: String,
    template_document_id: String,
    template_revision: u64,
    template_sha256: String,
    data_sha256: String,
    rows: usize,
    variants: Vec<String>,
    total: usize,
    render_profile: Value,
    font_backend: Value,
    items: Vec<ManifestItem>,
    input_digest: String,
    issues: Issues,
    export_recipes: Vec<ExportRecipe>,
}

fn plan_exports(
    doc: &DesignDocument,
    table: &csv::Table,
    req: &BatchRequest,
    directory: &File,
    recipes: &[ExportRecipe],
    resume: bool,
) -> Result<Vec<Planned>, Issues> {
    if recipes.is_empty() || recipes.len() > 5 {
        return Err(vec![Issue::error(
            Code::ExportUnsupported,
            "select one to five explicit export formats",
            "Choose PNG, JPEG, lossless WebP, SVG or RGB proof PDF.",
        )]);
    }
    let mut extensions = HashSet::new();
    for recipe in recipes {
        if !extensions.insert(recipe.format.extension()) {
            return Err(vec![Issue::error(
                Code::OutputCollision,
                "the same export format was selected twice",
                "Select each format once; separate jobs may use different settings.",
            )]);
        }
    }
    let substituted = req.name_pattern.replace("{format}", "png");
    let base_pattern = if let Some((stem, ext)) = substituted.rsplit_once('.') {
        if ext == "png" || recipes.iter().any(|r| r.format.extension() == ext) {
            format!("{stem}.png")
        } else {
            substituted.clone()
        }
    } else {
        substituted.clone()
    };
    let base_request = BatchRequest {
        template: req.template,
        data: req.data,
        variants: req.variants,
        name_pattern: &base_pattern,
        manifest_name: req.manifest_name,
        store: req.store,
        out_dir: req.out_dir,
        options: req.options,
    };
    let base = plan(doc, table, &base_request, directory, true)?;
    if base.len().checked_mul(recipes.len()).is_none_or(|count| count > 20_000) {
        return Err(vec![Issue::error(
            Code::ResourceLimit,
            "more than 20,000 row × variant × format items",
            "Split the dataset or select fewer formats and variants.",
        )]);
    }
    let mut names = HashSet::new();
    names.insert(req.manifest_name.to_ascii_lowercase());
    let mut issues = Vec::new();
    let mut planned = Vec::new();
    if !resume && io::exists(directory, req.manifest_name).map_err(|i| vec![i])? {
        issues.push(io::collision(req.manifest_name));
    }
    for item in base {
        for recipe in recipes {
            let extension = recipe.format.extension();
            let pattern = req.name_pattern.replace("{format}", extension);
            let raw = naming::expand_pattern(&pattern, &item.row_id, &item.variant_id).map_err(|i| vec![i])?;
            let file = if let Some((stem, ext)) = raw.rsplit_once('.') {
                if matches!(ext, "png" | "jpg" | "webp" | "svg" | "pdf") {
                    format!("{stem}.{extension}")
                } else {
                    raw
                }
            } else {
                raw
            };
            let mut p = item.clone();
            p.file = file;
            p.recipe = recipe.clone();
            if let Err(issue) = naming::validate_output_name(&p.file, extension) {
                issues.push(tag(issue, &p));
                continue;
            }
            if !names.insert(p.file.to_ascii_lowercase()) {
                issues.push(tag(
                    Issue::error(
                        Code::OutputCollision,
                        "two batch items have the same portable output name",
                        "Include rowId, variantId and format in the filename pattern.",
                    ),
                    &p,
                ));
                continue;
            }
            if !resume && io::exists(directory, &p.file).map_err(|i| vec![i])? {
                issues.push(tag(io::collision(&p.file), &p));
                continue;
            }
            planned.push(p);
        }
    }
    if issues.is_empty() { Ok(planned) } else { Err(issues) }
}

pub fn preflight(req: &BatchRequest) -> Result<BatchPreflight, Issues> {
    let directory = io::directory(req.out_dir).map_err(|i| vec![i])?;
    preflight_granted(req, &directory)
}
pub fn preflight_granted(req: &BatchRequest, directory: &File) -> Result<BatchPreflight, Issues> {
    preflight_internal(req, directory, false)
}
/// Inspect all rows before a resume selection. Existing-name exceptions here
/// never authorize publication; the resume command verifies journal and bytes.
pub fn preflight_resume_granted(req: &BatchRequest, directory: &File) -> Result<BatchPreflight, Issues> {
    preflight_internal(req, directory, true)
}
fn preflight_internal(req: &BatchRequest, directory: &File, resume: bool) -> Result<BatchPreflight, Issues> {
    preflight_export_granted(req, directory, &[ExportRecipe::default()], resume)
}
/// Inspect row × approved variant × explicitly selected derivative format.
pub fn preflight_export_granted(
    req: &BatchRequest,
    directory: &File,
    recipes: &[ExportRecipe],
    resume: bool,
) -> Result<BatchPreflight, Issues> {
    io::validate_directory(directory).map_err(|i| vec![i])?;
    let doc = load_document(req.template)?;
    let table = csv::parse(req.data).map_err(|i| vec![i])?;
    let planned = plan_exports(&doc, &table, req, directory, recipes, resume)?;
    let (summary, _) = inspect(&doc, &planned, req, table.records.len(), &AtomicBool::new(false), &mut |_| {});
    Ok(summary)
}

pub fn run_batch_controlled(req: &BatchRequest, cancelled: &AtomicBool, progress: impl FnMut(BatchProgress)) -> Result<Manifest, Issues> {
    let directory = io::directory(req.out_dir).map_err(|i| vec![i])?;
    run_batch_granted(req, &directory, cancelled, progress, ErrorPolicy::BlockAll, false)
}
pub fn resume_batch_controlled(
    req: &BatchRequest,
    cancelled: &AtomicBool,
    progress: impl FnMut(BatchProgress),
) -> Result<Manifest, Issues> {
    let directory = io::directory(req.out_dir).map_err(|i| vec![i])?;
    run_batch_granted(req, &directory, cancelled, progress, ErrorPolicy::BlockAll, true)
}

/// The host provides a retained local directory grant and explicit partial
/// policy. No output is overwritten; resume only adopts checksum-matching files
/// belonging to exactly this immutable input and engine profile.
pub fn run_batch_granted(
    req: &BatchRequest,
    directory: &File,
    cancelled: &AtomicBool,
    progress: impl FnMut(BatchProgress),
    policy: ErrorPolicy,
    resume: bool,
) -> Result<Manifest, Issues> {
    run_export_batch_granted(req, directory, cancelled, progress, policy, resume, &[ExportRecipe::default()])
}

/// Export recipes are part of the fixed job digest and every item record.
pub fn run_export_batch_granted(
    req: &BatchRequest,
    directory: &File,
    cancelled: &AtomicBool,
    mut progress: impl FnMut(BatchProgress),
    policy: ErrorPolicy,
    resume: bool,
    recipes: &[ExportRecipe],
) -> Result<Manifest, Issues> {
    io::validate_directory(directory).map_err(|i| vec![i])?;
    let doc = load_document(req.template)?;
    let table = csv::parse(req.data).map_err(|i| vec![i])?;
    let planned = plan_exports(&doc, &table, req, directory, recipes, resume)?;
    let digest = digest(req, policy, recipes);
    let stage_name = format!(".siliq-job-{digest}");
    let stage_exists = io::exists(directory, &stage_name).map_err(|i| vec![i])?;
    let final_exists = io::exists(directory, req.manifest_name).map_err(|i| vec![i])?;
    if !resume && stage_exists {
        return Err(vec![invalid("an interrupted job with these inputs already exists; use resume")]);
    }
    if resume && !stage_exists && !final_exists {
        return Err(vec![invalid("no journal or manifest matches this exact job snapshot")]);
    }
    let existing_stage = if stage_exists {
        Some(io::child(directory, &stage_name, false).map_err(|i| vec![i])?)
    } else {
        None
    };
    let _existing_lock = existing_stage.as_ref().map(lock).transpose()?;
    let mut original_manifest_sha = None;
    let mut journal = if resume && stage_exists {
        serde_json::from_slice::<Journal>(
            &io::read(existing_stage.as_ref().unwrap(), "journal.json", MAX_JOURNAL_BYTES).map_err(|i| vec![i])?,
        )
        .map_err(|_| vec![invalid("batch journal is malformed")])?
    } else if resume && final_exists {
        let bytes = io::read(directory, req.manifest_name, MAX_JOURNAL_BYTES).map_err(|i| vec![i])?;
        original_manifest_sha = Some(sha256_hex(&bytes));
        let saved: SavedManifest = serde_json::from_slice(&bytes).map_err(|_| vec![invalid("batch manifest is malformed")])?;
        if saved.input_digest != digest
            || saved.error_policy != policy.name()
            || saved.template_document_id != doc.document_id
            || saved.template_revision != doc.revision
            || saved.template_sha256 != sha256_hex(req.template)
            || saved.data_sha256 != sha256_hex(req.data)
            || saved.rows != table.records.len()
            || saved.variants != req.variants
            || saved.total != planned.len()
            || saved.render_profile != serde_json::to_value(RENDER_PROFILE).unwrap()
            || saved.font_backend != serde_json::to_value(req.options.font_backend).unwrap()
            || serde_json::to_value(&saved.export_recipes).unwrap() != serde_json::to_value(recipes).unwrap()
            || !matches!(saved.status.as_str(), "completed" | "completed_with_errors" | "cancelled")
        {
            return Err(vec![invalid("batch manifest belongs to different inputs, settings or engine")]);
        }
        Journal {
            version: 1,
            input_digest: digest.clone(),
            error_policy: policy,
            status: saved.status,
            published: saved.items.iter().map(|i| i.file.clone()).collect(),
            items: saved.items,
            issues: saved.issues,
        }
    } else {
        Journal {
            version: 1,
            input_digest: digest.clone(),
            error_policy: policy,
            status: "queued".into(),
            items: vec![],
            published: vec![],
            issues: vec![],
        }
    };
    if journal.version != 1
        || journal.input_digest != digest
        || journal.error_policy != policy
        || !matches!(
            journal.status.as_str(),
            "queued" | "ready" | "running" | "cancelled" | "failed" | "completed" | "completed_with_errors"
        )
    {
        return Err(vec![invalid("batch journal belongs to different inputs, settings or engine")]);
    }
    validate_saved_items(&journal, &planned, req)?;
    if resume && stage_exists && final_exists {
        let bytes = io::read(directory, req.manifest_name, MAX_JOURNAL_BYTES).map_err(|i| vec![i])?;
        let saved: SavedManifest =
            serde_json::from_slice(&bytes).map_err(|_| vec![invalid("existing manifest is not this job's manifest")])?;
        if saved.input_digest != digest {
            return Err(vec![io::collision(req.manifest_name)]);
        }
        original_manifest_sha = Some(sha256_hex(&bytes));
    }

    let (summary, expected) = inspect(&doc, &planned, req, table.records.len(), cancelled, &mut progress);
    if cancelled.load(Ordering::Acquire) {
        journal.status = "cancelled".into();
        if let Some(stage) = &existing_stage {
            persist(stage, &journal)?;
        } else {
            let stage = io::child(directory, &stage_name, true).map_err(|i| vec![i])?;
            let _lock = lock(&stage)?;
            persist(&stage, &journal)?;
        }
        progress(event("cancelled", planned.len(), journal.items.len(), None, vec![]));
        return Ok(manifest(
            req,
            &doc,
            table.records.len(),
            planned.len(),
            "cancelled",
            policy,
            journal.items,
            summary.issues,
            recipes,
        ));
    }
    if policy == ErrorPolicy::BlockAll && has_errors(&summary.issues) {
        return Err(summary.issues);
    }
    let stage = if let Some(stage) = existing_stage {
        stage
    } else {
        io::child(directory, &stage_name, true).map_err(|i| vec![i])?
    };
    let _new_lock = if stage_exists { None } else { Some(lock(&stage)?) };
    let mut staged_names: HashSet<String> = journal.items.iter().map(|item| item.file.clone()).collect();
    // Valid-only resume must not publish previously prepared items that fail
    // the current all-item native preflight. Existing final files are kept.
    journal.items.retain(|item| expected.contains_key(&item.file));
    journal.published.retain(|file| expected.contains_key(file));
    journal.issues = summary.issues;
    journal.status = "running".into();
    persist(&stage, &journal)?;
    let mut completed = 0;
    for p in &planned {
        if cancelled.load(Ordering::Acquire) {
            return cancel(
                req,
                &doc,
                table.records.len(),
                planned.len(),
                policy,
                &stage,
                journal,
                &mut progress,
                recipes,
            );
        }
        let Some(expected_item) = expected.get(&p.file) else {
            completed += 1;
            progress(event(
                "running",
                planned.len(),
                completed,
                Some(p),
                summary_item_issues(&summary.items, &p.file),
            ));
            continue;
        };
        if let Some(prior) = journal.items.iter().find(|item| item.file == p.file)
            && (prior.sha256 != expected_item.sha256 || prior.width != expected_item.width || prior.height != expected_item.height)
        {
            return Err(vec![checksum(p)]);
        }
        if io::exists(directory, &p.file).map_err(|i| vec![i])? {
            if !resume || !journal.items.iter().any(|item| item.file == p.file) {
                return Err(vec![io::collision(&p.file)]);
            }
            verify(directory, expected_item, Some(&stage))?;
        } else if io::exists(&stage, &p.file).map_err(|i| vec![i])? {
            verify(&stage, expected_item, Some(directory))?;
        } else {
            let output = produce(&doc, p, req)?;
            if output.export_report.output_sha256 != expected_item.sha256 {
                return Err(vec![checksum(p)]);
            }
            io::new_file(&stage, &p.file, &output.bytes).map_err(|i| vec![tag(i, p)])?;
            verify(&stage, expected_item, Some(directory))?;
        }
        if let Some(index) = journal.items.iter().position(|item| item.file == p.file) {
            journal.items[index] = expected_item.clone();
        } else {
            journal.items.push(expected_item.clone());
        }
        staged_names.insert(p.file.clone());
        persist(&stage, &journal)?;
        completed += 1;
        progress(event("running", planned.len(), completed, Some(p), expected_item.warnings.clone()));
    }
    journal.status = "ready".into();
    persist(&stage, &journal)?;
    let publishing = journal.items.clone();
    for item in &publishing {
        if cancelled.load(Ordering::Acquire) {
            return cancel(
                req,
                &doc,
                table.records.len(),
                planned.len(),
                policy,
                &stage,
                journal,
                &mut progress,
                recipes,
            );
        }
        if io::exists(directory, &item.file).map_err(|i| vec![i])? {
            if !resume {
                return Err(vec![io::collision(&item.file)]);
            }
            verify(directory, item, Some(&stage))?;
        } else {
            verify(&stage, item, Some(directory))?;
            io::publish(&stage, directory, &item.file).map_err(|i| vec![i])?;
        }
    }
    journal.published = journal.items.iter().map(|i| i.file.clone()).collect();
    let status = if has_errors(&journal.issues) {
        "completed_with_errors"
    } else {
        "completed"
    };
    journal.status = status.into();
    persist(&stage, &journal)?;
    let result = manifest(
        req,
        &doc,
        table.records.len(),
        planned.len(),
        status,
        policy,
        journal.items.clone(),
        journal.issues.clone(),
        recipes,
    );
    if let Some(sha) = original_manifest_sha
        && sha256_hex(&io::read(directory, req.manifest_name, MAX_JOURNAL_BYTES).map_err(|i| vec![i])?) != sha
    {
        return Err(vec![invalid("existing job manifest changed during resume")]);
    }
    let bytes = serde_json::to_vec_pretty(&result).map_err(|e| vec![io::error(e)])?;
    io::atomic_json(directory, req.manifest_name, &bytes, resume && final_exists).map_err(|i| vec![i])?;
    // Cleanup only known app-owned files relative to the held descriptor.
    for name in &staged_names {
        io::remove(&stage, name);
    }
    io::remove(&stage, "journal.json");
    io::remove(&stage, "lock");
    io::remove_child(directory, &stage_name);
    progress(event(status, planned.len(), planned.len(), None, result.issues.clone()));
    Ok(result)
}

fn inspect(
    doc: &DesignDocument,
    planned: &[Planned],
    req: &BatchRequest,
    rows: usize,
    cancelled: &AtomicBool,
    progress: &mut impl FnMut(BatchProgress),
) -> (BatchPreflight, BTreeMap<String, ManifestItem>) {
    let mut items = Vec::new();
    let mut issues = Vec::new();
    let mut metadata = BTreeMap::new();
    progress(event("preflighting", planned.len(), 0, None, vec![]));
    for p in planned {
        if cancelled.load(Ordering::Acquire) {
            break;
        }
        let findings = match produce(doc, p, req) {
            Ok(output) => {
                let item = output_item(p, req, &output);
                let warnings = item.warnings.clone();
                metadata.insert(p.file.clone(), item);
                warnings
            }
            Err(errors) => errors,
        };
        issues.extend(findings.iter().cloned());
        items.push(PlannedItem {
            row_id: p.row_id.clone(),
            variant_id: p.variant_id.clone(),
            file: p.file.clone(),
            issues: findings.clone(),
            format: p.recipe.format,
        });
        progress(event("preflighting", planned.len(), items.len(), Some(p), findings));
    }
    (
        BatchPreflight {
            rows,
            total: planned.len(),
            variant_ids: req.variants.to_vec(),
            items,
            issues,
        },
        metadata,
    )
}
struct PreparedOutput {
    bytes: Vec<u8>,
    report: RenderReport,
    export_report: ExportReport,
}
fn produce(doc: &DesignDocument, p: &Planned, req: &BatchRequest) -> Result<PreparedOutput, Issues> {
    let bound = bind_row(doc, &p.row_id, &p.values).map_err(|issues| issues.into_iter().map(|i| tag(i, p)).collect::<Issues>())?;
    let rendered = render_document(&bound, &p.variant_id, req.store, req.options)
        .map_err(|issues| issues.into_iter().map(|i| tag(i, p)).collect::<Issues>())?;
    if p.recipe.format != ExportFormat::Png
        && u64::from(rendered.report.width) * u64::from(rendered.report.height) * 4 * 5 > 1024 * 1024 * 1024
    {
        return Err(vec![tag(
            Issue::error(
                Code::ResourceLimit,
                "derivative export would exceed the 1 GiB working-set reservation",
                "Use a smaller variant for this export format.",
            ),
            p,
        )]);
    }
    let exported = design_export::export(&rendered, &p.recipe).map_err(|i| vec![tag(i, p)])?;
    Ok(PreparedOutput {
        bytes: exported.bytes,
        report: rendered.report,
        export_report: exported.report,
    })
}
fn output_item(p: &Planned, req: &BatchRequest, out: &PreparedOutput) -> ManifestItem {
    ManifestItem {
        row_id: p.row_id.clone(),
        variant_id: p.variant_id.clone(),
        file: p.file.clone(),
        sha256: out.export_report.output_sha256.clone(),
        width: out.report.width,
        height: out.report.height,
        input_digest: item_digest(req, p),
        applied_font_sizes: out.report.text.iter().map(|t| (t.node_id.clone(), t.applied_font_size)).collect(),
        warnings: out.report.warnings.iter().cloned().map(|i| tag(i, p)).collect(),
        format: p.recipe.format,
        export_report: serde_json::to_value(&out.export_report).unwrap(),
    }
}
fn item_digest(req: &BatchRequest, p: &Planned) -> String {
    sha256_hex(
        &serde_json::to_vec(&(
            sha256_hex(req.template),
            &p.row_id,
            &p.values,
            &p.variant_id,
            &p.file,
            RENDER_PROFILE,
            req.options.font_backend,
            &p.recipe,
        ))
        .unwrap(),
    )
}
fn digest(req: &BatchRequest, policy: ErrorPolicy, recipes: &[ExportRecipe]) -> String {
    sha256_hex(
        &serde_json::to_vec(&(
            sha256_hex(req.template),
            sha256_hex(req.data),
            req.variants,
            req.name_pattern,
            req.manifest_name,
            RENDER_PROFILE,
            req.options.font_backend,
            policy,
            recipes,
        ))
        .unwrap(),
    )
}
/// Portable job identity, including immutable document/data, recipe and native
/// engine profile. It grants no output authority by itself.
pub fn batch_input_digest(req: &BatchRequest, policy: ErrorPolicy) -> String {
    digest(req, policy, &[ExportRecipe::default()])
}
pub fn export_batch_input_digest(req: &BatchRequest, policy: ErrorPolicy, recipes: &[ExportRecipe]) -> String {
    digest(req, policy, recipes)
}
// The checkpoint builder names all snapshot fields explicitly at the callsite.
#[allow(clippy::too_many_arguments)]
fn manifest(
    req: &BatchRequest,
    doc: &DesignDocument,
    rows: usize,
    total: usize,
    status: &'static str,
    policy: ErrorPolicy,
    items: Vec<ManifestItem>,
    issues: Issues,
    recipes: &[ExportRecipe],
) -> Manifest {
    Manifest {
        status,
        error_policy: policy.name(),
        template_document_id: doc.document_id.clone(),
        template_revision: doc.revision,
        template_sha256: sha256_hex(req.template),
        data_sha256: sha256_hex(req.data),
        rows,
        variants: req.variants.to_vec(),
        total,
        render_profile: RENDER_PROFILE,
        font_backend: req.options.font_backend,
        items,
        input_digest: digest(req, policy, recipes),
        issues,
        export_recipes: recipes.to_vec(),
    }
}
fn validate_saved_items(journal: &Journal, planned: &[Planned], req: &BatchRequest) -> Result<(), Issues> {
    let mut seen = HashSet::new();
    if journal.items.len() > planned.len() || journal.issues.len() > 100_000 {
        return Err(vec![invalid("batch journal exceeds its item limits")]);
    }
    for item in &journal.items {
        let Some(p) = planned
            .iter()
            .find(|p| p.file == item.file && p.row_id == item.row_id && p.variant_id == item.variant_id)
        else {
            return Err(vec![invalid("batch journal contains an unknown output")]);
        };
        if !seen.insert(&item.file)
            || item.input_digest != item_digest(req, p)
            || item.format != p.recipe.format
            || !design_model::validate::is_sha256_hex(&item.sha256)
            || item.width == 0
            || item.height == 0
        {
            return Err(vec![invalid("batch journal item identity or digest is invalid")]);
        }
    }
    if journal
        .published
        .iter()
        .any(|file| !journal.items.iter().any(|item| &item.file == file))
    {
        return Err(vec![invalid("batch publication journal is inconsistent")]);
    }
    Ok(())
}
fn persist(stage: &File, journal: &Journal) -> Result<(), Issues> {
    let bytes = serde_json::to_vec_pretty(journal).map_err(|e| vec![io::error(e)])?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(vec![Issue::error(
            Code::ResourceLimit,
            "batch journal exceeds 20 MiB",
            "Split the dataset into smaller jobs.",
        )]);
    }
    io::atomic_json(stage, "journal.json", &bytes, true).map_err(|i| vec![i])
}
fn verify(directory: &File, item: &ManifestItem, peer: Option<&File>) -> Result<(), Issues> {
    let bytes = io::read_with_peer(directory, &item.file, MAX_PNG_BYTES, peer).map_err(|i| {
        vec![
            i.row(&item.row_id)
                .variant(&item.variant_id)
                .export_format(format_name(item.format)),
        ]
    })?;
    if sha256_hex(&bytes) != item.sha256 {
        return Err(vec![
            Issue::error(
                Code::AssetChecksum,
                "existing output bytes do not match this job's recorded checksum",
                "Keep the existing file and select a new output directory; checksum-mismatched files are never overwritten.",
            )
            .row(&item.row_id)
            .variant(&item.variant_id)
            .export_format(format_name(item.format)),
        ]);
    }
    Ok(())
}
fn lock(stage: &File) -> Result<File, Issues> {
    use rustix::fs::{FileType, FlockOperation, Mode, OFlags};
    let file: File = rustix::fs::openat(
        stage,
        "lock",
        OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|e| vec![io::error(e)])?
    .into();
    let stat = rustix::fs::fstat(&file).map_err(|e| vec![io::error(e)])?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_nlink != 1 {
        return Err(vec![io::denied()]);
    }
    rustix::fs::flock(&file, FlockOperation::NonBlockingLockExclusive).map_err(|_| vec![invalid("this batch is already running")])?;
    Ok(file)
}
#[allow(clippy::too_many_arguments)]
fn cancel(
    req: &BatchRequest,
    doc: &DesignDocument,
    rows: usize,
    total: usize,
    policy: ErrorPolicy,
    stage: &File,
    mut journal: Journal,
    progress: &mut impl FnMut(BatchProgress),
    recipes: &[ExportRecipe],
) -> Result<Manifest, Issues> {
    journal.status = "cancelled".into();
    persist(stage, &journal)?;
    progress(event("cancelled", total, journal.items.len(), None, vec![]));
    Ok(manifest(
        req,
        doc,
        rows,
        total,
        "cancelled",
        policy,
        journal.items,
        journal.issues,
        recipes,
    ))
}
fn tag(mut issue: Issue, p: &Planned) -> Issue {
    issue.location.row_id.get_or_insert_with(|| p.row_id.clone());
    issue.location.variant_id.get_or_insert_with(|| p.variant_id.clone());
    issue
        .location
        .export_format
        .get_or_insert_with(|| format_name(p.recipe.format).to_owned());
    issue
}
fn format_name(format: ExportFormat) -> &'static str {
    match format {
        ExportFormat::Png => "png",
        ExportFormat::Jpeg => "jpeg",
        ExportFormat::Webp => "webp",
        ExportFormat::Svg => "svg",
        ExportFormat::Pdf => "pdf",
    }
}
fn checksum(p: &Planned) -> Issue {
    tag(
        Issue::error(
            Code::AssetChecksum,
            "batch output does not match the fixed preflight snapshot",
            "Use the same engine, resources and input snapshot when resuming.",
        ),
        p,
    )
}
fn invalid(message: &str) -> Issue {
    Issue::error(
        Code::DataInvalid,
        message,
        "Resume only the original immutable batch snapshot, or start a new job in an empty directory.",
    )
}
fn event(status: &str, total: usize, completed: usize, item: Option<&Planned>, issues: Issues) -> BatchProgress {
    BatchProgress {
        status: status.into(),
        total,
        completed,
        current_row_id: item.map(|p| p.row_id.clone()),
        current_variant_id: item.map(|p| p.variant_id.clone()),
        issues,
    }
}
fn summary_item_issues(items: &[PlannedItem], name: &str) -> Issues {
    items
        .iter()
        .find(|item| item.file == name)
        .map_or_else(Vec::new, |item| item.issues.clone())
}
