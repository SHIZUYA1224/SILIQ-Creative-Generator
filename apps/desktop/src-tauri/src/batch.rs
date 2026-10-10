//! Dataset and job handles owned by the native host. The WebView sees IDs and
//! typed mappings, never source/output paths or mutable worker inputs.
use crate::{
    blocking,
    grants::{self, DirectoryGrant},
    host::{HostResult, SharedHost, error, opaque_id},
    lock,
};
use design_model::{Code, Issue};
use design_render::{RenderOptions, resources::ResourceStore};
use serde::{Deserialize, Serialize};
use skia_safe::{
    AlphaType, ColorSpace, ColorType, Data, FilterMode, ImageInfo, MipmapMode, Paint, Rect, SamplingOptions, images, png_encoder, surfaces,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::State;
use tauri_plugin_dialog::DialogExt;

pub type SharedBatches = Arc<Mutex<BatchHost>>;
// Gallery requests may arrive in parallel. Only one full native raster is
// allocated at a time before reducing it to a maximum 512 × 512 PNG.
static PREVIEW_GATE: Mutex<()> = Mutex::new(());
const MAX_PREVIEW_SIDE: u32 = 512;
const MAX_PREVIEW_PNG_BYTES: usize = 2 * 1024 * 1024;
#[derive(Default)]
pub struct BatchHost {
    datasets: BTreeMap<String, Dataset>,
    plans: BTreeMap<String, Arc<Plan>>,
    jobs: BTreeMap<String, Arc<Job>>,
    started: BTreeMap<String, String>,
}
struct Dataset {
    session_id: String,
    table: job_runner::csv::Table,
}
struct Plan {
    template: Vec<u8>,
    data: Vec<u8>,
    variants: Vec<String>,
    name_pattern: String,
    resources: ResourceStore,
    output: DirectoryGrant,
    policy: job_runner::ErrorPolicy,
    recipes: Vec<design_export::ExportRecipe>,
    source_workspace: std::path::PathBuf,
    snapshot_path: Option<std::path::PathBuf>,
    items: Vec<job_runner::PlannedItem>,
    can_start: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedSnapshot {
    snapshot_id: String,
    document_title: String,
    revision: u64,
    rows: usize,
    variants: Vec<String>,
    name_pattern: String,
    error_policy: job_runner::ErrorPolicy,
    template_sha256: String,
    data_sha256: String,
    #[serde(default = "default_recipes")]
    recipes: Vec<design_export::ExportRecipe>,
    #[serde(default)]
    status: String,
}
fn default_recipes() -> Vec<design_export::ExportRecipe> {
    vec![design_export::ExportRecipe::default()]
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedBatchView {
    snapshot_id: String,
    document_title: String,
    revision: u64,
    rows: usize,
    total: usize,
    variant_ids: Vec<String>,
    format_ids: Vec<design_export::ExportFormat>,
    error_policy: job_runner::ErrorPolicy,
}
impl SavedSnapshot {
    fn view(&self) -> SavedBatchView {
        SavedBatchView {
            snapshot_id: self.snapshot_id.clone(),
            document_title: self.document_title.clone(),
            revision: self.revision,
            rows: self.rows,
            total: self.rows * self.variants.len() * self.recipes.len(),
            variant_ids: self.variants.clone(),
            format_ids: self.recipes.iter().map(|r| r.format).collect(),
            error_policy: self.error_policy,
        }
    }
}
struct Job {
    plan: Arc<Plan>,
    cancel: AtomicBool,
    status: Mutex<JobView>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetView {
    dataset_id: String,
    columns: Vec<String>,
    row_count: usize,
    sample_rows: Vec<BTreeMap<String, String>>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    plan_id: Option<String>,
    preview_plan_id: Option<String>,
    preview_kind: &'static str,
    rows: usize,
    total: usize,
    variant_ids: Vec<String>,
    preview_names: Vec<String>,
    items: Vec<job_runner::PlannedItem>,
    error_policy: job_runner::ErrorPolicy,
    issues: Vec<Issue>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    job_id: String,
    status: String,
    total: usize,
    completed: usize,
    current_row_id: Option<String>,
    current_variant_id: Option<String>,
    issues: Vec<Issue>,
}
fn batch_lock(shared: &SharedBatches) -> HostResult<std::sync::MutexGuard<'_, BatchHost>> {
    shared.lock().map_err(|_| {
        error(
            Code::RenderFailed,
            "batch host is unavailable",
            "Restart the app and inspect the saved job manifest.",
        )
    })
}

impl BatchHost {
    pub(crate) fn protected_paths(&self) -> std::collections::BTreeSet<std::path::PathBuf> {
        let mut paths = std::collections::BTreeSet::new();
        for (id, plan) in &self.plans {
            let finished = self.started.get(id).and_then(|job| self.jobs.get(job)).is_some_and(|job| {
                job.status
                    .lock()
                    .is_ok_and(|view| matches!(view.status.as_str(), "completed" | "completed_with_errors"))
            });
            if !finished {
                paths.insert(plan.source_workspace.clone());
                if let Some(path) = &plan.snapshot_path {
                    paths.insert(path.clone());
                }
            }
        }
        for job in self.jobs.values() {
            let finished = job
                .status
                .lock()
                .is_ok_and(|view| matches!(view.status.as_str(), "completed" | "completed_with_errors"));
            if !finished {
                paths.insert(job.plan.source_workspace.clone());
                if let Some(path) = &job.plan.snapshot_path {
                    paths.insert(path.clone());
                }
            }
        }
        paths
    }
}

pub(crate) fn completed_snapshot(root: &std::path::Path, id: &str) -> bool {
    read_snapshot(root, id).is_ok_and(|(saved, _)| matches!(saved.status.as_str(), "completed" | "completed_with_errors"))
}
fn mark_snapshot(path: &std::path::Path, status: &str) -> HostResult<()> {
    let id = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| error(Code::SaveFailed, "invalid snapshot workspace", "Preserve the original snapshot."))?;
    let root = path
        .parent()
        .ok_or_else(|| error(Code::SaveFailed, "invalid snapshot workspace", "Preserve the original snapshot."))?;
    let (mut saved, _) = read_snapshot(root, id)?;
    saved.status = status.to_owned();
    DirectoryGrant::new(path.to_owned())
        .and_then(|g| g.replace_control("snapshot.json", &serde_json::to_vec(&saved).expect("snapshot metadata")))
        .map_err(|i| vec![i])
}

#[tauri::command]
pub async fn select_dataset(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    batches: State<'_, SharedBatches>,
    session_id: String,
) -> HostResult<Option<DatasetView>> {
    let shared = state.inner().clone();
    let shared_batches = batches.inner().clone();
    blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .add_filter("UTF-8 CSV / JSON", &["csv", "json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local CSV or JSON files are supported",
                "Choose a local UTF-8 CSV or JSON file.",
            )
        })?;
        select_dataset_path(&shared, &shared_batches, session_id, path).map(Some)
    })
    .await
}

pub(crate) fn select_dataset_path(
    shared: &SharedHost,
    shared_batches: &SharedBatches,
    session_id: String,
    path: std::path::PathBuf,
) -> HostResult<DatasetView> {
    lock(shared)?.record(&session_id)?;
    let bytes = grants::read_selected(&path, job_runner::csv::MAX_CSV_BYTES as u64).map_err(|i| vec![i])?;
    let json = path.extension().is_some_and(|s| s.eq_ignore_ascii_case("json"));
    let table = parse_dataset(&bytes, json)?;
    let id = opaque_id("dataset");
    let view = DatasetView {
        dataset_id: id.clone(),
        columns: table.header.clone(),
        row_count: table.records.len(),
        sample_rows: table
            .records
            .iter()
            .take(5)
            .map(|(_, row)| table.header.iter().cloned().zip(row.iter().cloned()).collect())
            .collect(),
    };
    let mut batches = batch_lock(shared_batches)?;
    if batches.datasets.len() >= 32 {
        return Err(error(
            Code::ResourceLimit,
            "too many datasets are open",
            "Restart the app to release unused dataset snapshots.",
        ));
    }
    batches.datasets.insert(id, Dataset { session_id, table });
    Ok(view)
}

struct JsonRow(BTreeMap<String, String>);
impl<'de> Deserialize<'de> for JsonRow {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RowVisitor;
        impl<'de> serde::de::Visitor<'de> for RowVisitor {
            type Value = JsonRow;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an object with unique non-empty keys and string values")
            }
            fn visit_map<M: serde::de::MapAccess<'de>>(self, mut map: M) -> Result<JsonRow, M::Error> {
                let mut row = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    if key.is_empty() || row.contains_key(&key) {
                        return Err(serde::de::Error::custom("JSON column names must be non-empty and unique"));
                    }
                    // String deserialization refuses numeric, null, boolean,
                    // array and object values; there is no value coercion.
                    row.insert(key, map.next_value::<String>()?);
                }
                Ok(JsonRow(row))
            }
        }
        deserializer.deserialize_map(RowVisitor)
    }
}

const JSON_ROW_LIMIT_MESSAGE: &str = "JSON dataset exceeds the 10000-row limit";
struct JsonRows(Vec<JsonRow>);
impl<'de> Deserialize<'de> for JsonRows {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct RowsVisitor;
        impl<'de> serde::de::Visitor<'de> for RowsVisitor {
            type Value = JsonRows;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an array of objects with string values")
            }
            fn visit_seq<S: serde::de::SeqAccess<'de>>(self, mut seq: S) -> Result<JsonRows, S::Error> {
                let mut rows = Vec::new();
                while let Some(row) = seq.next_element::<JsonRow>()? {
                    if rows.len() >= job_runner::csv::MAX_ROWS {
                        return Err(serde::de::Error::custom(JSON_ROW_LIMIT_MESSAGE));
                    }
                    rows.push(row);
                }
                Ok(JsonRows(rows))
            }
        }
        deserializer.deserialize_seq(RowsVisitor)
    }
}

fn parse_dataset(bytes: &[u8], force_json: bool) -> HostResult<job_runner::csv::Table> {
    if bytes.len() > job_runner::csv::MAX_CSV_BYTES {
        return Err(error(
            Code::ResourceLimit,
            "dataset exceeds the 20 MiB limit",
            "Split the data into smaller files.",
        ));
    }
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let first = bytes.iter().copied().find(|b| !b.is_ascii_whitespace());
    let table = if force_json || matches!(first, Some(b'[' | b'{')) {
        let JsonRows(rows) = serde_json::from_slice::<JsonRows>(bytes).map_err(|e| {
            error(
                if e.to_string().starts_with(JSON_ROW_LIMIT_MESSAGE) {
                    Code::ResourceLimit
                } else {
                    Code::DataInvalid
                },
                format!("JSON dataset: {e}"),
                "Use an array of objects with identical columns and string values only.",
            )
        })?;
        let header: Vec<String> = rows.first().map(|row| row.0.keys().cloned().collect()).unwrap_or_default();
        if header.is_empty() {
            return Err(error(
                Code::DataInvalid,
                "JSON dataset has no columns or rows",
                "Add objects with named string fields.",
            ));
        }
        let mut records = Vec::with_capacity(rows.len());
        for (index, JsonRow(row)) in rows.into_iter().enumerate() {
            if row.keys().ne(header.iter()) {
                return Err(error(
                    Code::DataInvalid,
                    format!("JSON row {} has different columns", index + 1),
                    "Every row must contain exactly the same field names.",
                ));
            }
            records.push((index + 2, header.iter().map(|key| row[key].clone()).collect()));
        }
        job_runner::csv::Table { header, records }
    } else {
        job_runner::csv::parse(bytes).map_err(|i| vec![i])?
    };
    if table.records.is_empty() {
        return Err(error(
            Code::DataInvalid,
            "dataset contains no data rows",
            "Add at least one data row.",
        ));
    }
    Ok(table)
}

fn mapped_csv(table: &job_runner::csv::Table, mapping: &BTreeMap<String, String>) -> HostResult<Vec<u8>> {
    if mapping.is_empty() {
        return Err(error(
            Code::DataInvalid,
            "no dataset columns have been mapped",
            "Map a rowId column and the required template slots.",
        ));
    }
    let mut targets = std::collections::BTreeSet::new();
    for (column, target) in mapping {
        if !table.header.contains(column) {
            return Err(error(
                Code::DataInvalid,
                "mapping references an unknown dataset column",
                "Choose a column from the selected dataset.",
            ));
        }
        if !targets.insert(target) {
            return Err(error(
                Code::DataInvalid,
                "two dataset columns map to the same target",
                "Map each slot and rowId at most once.",
            ));
        }
    }
    if !targets.contains(&"rowId".to_owned()) {
        return Err(error(
            Code::DataInvalid,
            "rowId is not mapped",
            "Map a column containing stable unique row IDs.",
        ));
    }
    let mut columns: Vec<_> = table
        .header
        .iter()
        .enumerate()
        .filter_map(|(i, h)| mapping.get(h).map(|target| (i, target)))
        .collect();
    // Mapping establishes a canonical rowId-first, slot-name order for both
    // source formats. Object key order or CSV column order cannot alter it.
    columns.sort_by(|(_, a), (_, b)| (a.as_str() != "rowId", a.as_str()).cmp(&(b.as_str() != "rowId", b.as_str())));
    let quote = |value: &str| format!("\"{}\"", value.replace('"', "\"\""));
    let mut csv = columns.iter().map(|(_, target)| quote(target)).collect::<Vec<_>>().join(",");
    csv.push('\n');
    for (_, row) in &table.records {
        csv.push_str(&columns.iter().map(|(i, _)| quote(&row[*i])).collect::<Vec<_>>().join(","));
        csv.push('\n');
        if csv.len() > job_runner::csv::MAX_CSV_BYTES {
            return Err(error(
                Code::ResourceLimit,
                "mapped dataset exceeds the 20 MiB limit",
                "Split the data into smaller files.",
            ));
        }
    }
    let parsed = job_runner::csv::parse(csv.as_bytes()).map_err(|i| vec![i])?;
    for ((_, source), (_, canonical)) in table.records.iter().zip(&parsed.records) {
        if columns.iter().map(|(i, _)| &source[*i]).ne(canonical.iter()) {
            return Err(error(
                Code::DataInvalid,
                "dataset text cannot be preserved by the canonical CSV format",
                "Use LF line breaks inside text values; carriage returns are not converted automatically.",
            ));
        }
    }
    Ok(csv.into_bytes())
}

#[tauri::command]
// Keep the established, typed frontend argument names at the IPC boundary.
#[allow(clippy::too_many_arguments)]
pub async fn preflight_batch(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    batches: State<'_, SharedBatches>,
    session_id: String,
    revision: u64,
    dataset_id: String,
    mapping: BTreeMap<String, String>,
    variant_ids: Vec<String>,
    name_pattern: String,
    error_policy: Option<job_runner::ErrorPolicy>,
    recipes: Option<Vec<design_export::ExportRecipe>>,
) -> HostResult<Option<PlanView>> {
    let shared = state.inner().clone();
    let shared_batches = batches.inner().clone();
    blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("Choose a fresh batch output folder")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let output_directory = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local output folders are supported",
                "Choose a local directory.",
            )
        })?;
        preflight_batch_path(
            &shared,
            &shared_batches,
            PreflightBatchArgs {
                session_id,
                revision,
                dataset_id,
                mapping,
                variant_ids,
                name_pattern,
                error_policy,
                recipes,
                output_directory,
            },
        )
        .map(Some)
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PreflightBatchArgs {
    pub session_id: String,
    pub revision: u64,
    pub dataset_id: String,
    pub mapping: BTreeMap<String, String>,
    pub variant_ids: Vec<String>,
    pub name_pattern: String,
    pub error_policy: Option<job_runner::ErrorPolicy>,
    pub recipes: Option<Vec<design_export::ExportRecipe>>,
    pub output_directory: std::path::PathBuf,
}

pub(crate) fn preflight_batch_path(shared: &SharedHost, shared_batches: &SharedBatches, args: PreflightBatchArgs) -> HostResult<PlanView> {
    let PreflightBatchArgs {
        session_id,
        revision,
        dataset_id,
        mapping,
        variant_ids,
        name_pattern,
        error_policy,
        recipes,
        output_directory,
    } = args;
    let (template, resources, source_workspace, _lease) = {
        let mut host = lock(shared)?;
        let record = host.record(&session_id)?;
        record.check_revision(revision)?;
        let template = serde_json::to_vec(record.session.document()).expect("validated document");
        let resources = record.resources.clone();
        let source_workspace = record.workspace.clone();
        let lease = host.pin_workspace(shared.clone(), source_workspace.clone());
        (template, resources, source_workspace, lease)
    };
    let data = {
        let batches = batch_lock(shared_batches)?;
        let dataset = batches
            .datasets
            .get(&dataset_id)
            .filter(|d| d.session_id == session_id)
            .ok_or_else(|| {
                error(
                    Code::PathDenied,
                    "dataset belongs to another or closed session",
                    "Select this document's CSV again.",
                )
            })?;
        mapped_csv(&dataset.table, &mapping)?
    };
    let output = DirectoryGrant::new(output_directory).map_err(|i| vec![i])?;
    let policy = error_policy.unwrap_or(job_runner::ErrorPolicy::BlockAll);
    let recipes = recipes.unwrap_or_else(default_recipes);
    if recipes.is_empty() || recipes.len() > 5 {
        return Err(error(
            Code::ResourceLimit,
            "batch must select between one and five export recipes",
            "Choose PNG, JPEG, WebP, SVG or PDF at most once each.",
        ));
    }
    let mut plan = Plan {
        template,
        data,
        variants: variant_ids.clone(),
        name_pattern,
        resources,
        output,
        policy,
        recipes,
        source_workspace,
        snapshot_path: None,
        items: Vec::new(),
        can_start: false,
    };
    let request = plan.request();
    match job_runner::preflight_export_granted(&request, plan.output.directory(), &plan.recipes, false) {
        Ok(preflight) => {
            let id = opaque_id("plan");
            let ready = policy == job_runner::ErrorPolicy::ValidOnly || !design_model::has_errors(&preflight.issues);
            let view = PlanView {
                plan_id: ready.then(|| id.clone()),
                preview_plan_id: Some(id.clone()),
                preview_kind: "inspection-png",
                rows: preflight.rows,
                total: preflight.total,
                variant_ids,
                preview_names: preflight.items.iter().take(30).map(|i| i.file.clone()).collect(),
                items: preflight.items.clone(),
                error_policy: policy,
                issues: preflight.issues,
            };
            plan.items = preflight.items;
            plan.can_start = ready;
            if ready {
                let snapshot_root = lock(shared)?.workspace.join("batch-snapshots");
                save_snapshot(&snapshot_root, &id, &plan, preflight.rows)?;
                plan.snapshot_path = Some(snapshot_root.join(&id));
            }
            batch_lock(shared_batches)?.plans.insert(id, Arc::new(plan));
            Ok(view)
        }
        Err(issues) => Ok(PlanView {
            plan_id: None,
            preview_plan_id: None,
            preview_kind: "inspection-png",
            rows: job_runner::csv::parse(&plan.data).map_or(0, |t| t.records.len()),
            total: 0,
            variant_ids,
            preview_names: Vec::new(),
            items: Vec::new(),
            error_policy: policy,
            issues,
        }),
    }
}

/// A bounded inspection thumbnail of one frozen row × variant × format item.
/// All formats share the approved native raster; format conversion is described
/// by the export report. No output is published or new file grant is requested.
#[tauri::command]
pub async fn request_batch_preview(
    state: State<'_, SharedHost>,
    batches: State<'_, SharedBatches>,
    plan_id: String,
    item_index: usize,
) -> HostResult<tauri::ipc::Response> {
    let shared = state.inner().clone();
    let shared_batches = batches.inner().clone();
    blocking(move || batch_preview_plain(&shared, &shared_batches, plan_id, item_index).map(tauri::ipc::Response::new)).await
}

pub(crate) fn batch_preview_plain(
    shared: &SharedHost,
    shared_batches: &SharedBatches,
    plan_id: String,
    item_index: usize,
) -> HostResult<Vec<u8>> {
    let plan = batch_lock(shared_batches)?
        .plans
        .get(&plan_id)
        .cloned()
        .ok_or_else(|| error(Code::PathDenied, "unknown frozen preview plan", "Run the batch inspection again."))?;
    // A completed plan can become eligible for explicit storage cleanup.
    // Hold leases before touching any cached resource bytes.
    let (_source_lease, _snapshot_lease) = {
        let mut host = lock(shared)?;
        let source = host.pin_workspace(shared.clone(), plan.source_workspace.clone());
        let snapshot = plan.snapshot_path.as_ref().map(|p| host.pin_workspace(shared.clone(), p.clone()));
        (source, snapshot)
    };
    let _permit = PREVIEW_GATE.lock().map_err(|_| {
        error(
            Code::RenderFailed,
            "candidate preview renderer is unavailable",
            "Restart the app and inspect the batch again.",
        )
    })?;
    render_batch_thumbnail(&plan, item_index)
}

fn render_batch_thumbnail(plan: &Plan, item_index: usize) -> HostResult<Vec<u8>> {
    let item = plan.items.get(item_index).ok_or_else(|| {
        error(
            Code::DataInvalid,
            "candidate index is outside the frozen plan",
            "Choose one listed candidate.",
        )
    })?;
    if design_model::has_errors(&item.issues) {
        return Err(item.issues.clone());
    }
    let doc = design_model::load_document(&plan.template)?;
    let table = job_runner::csv::parse(&plan.data).map_err(|i| vec![i])?;
    let id_column = table
        .header
        .iter()
        .position(|h| h == job_runner::csv::ROW_ID_COLUMN)
        .ok_or_else(|| error(Code::DataInvalid, "frozen dataset has no rowId", "Inspect the data mapping again."))?;
    let (_, row) = table.records.iter().find(|(_, r)| r[id_column] == item.row_id).ok_or_else(|| {
        error(
            Code::DataInvalid,
            "candidate row is absent from the frozen dataset",
            "Inspect the data mapping again.",
        )
    })?;
    let values = table
        .header
        .iter()
        .zip(row)
        .filter(|(h, _)| *h != job_runner::csv::ROW_ID_COLUMN)
        .map(|(h, v)| (h.clone(), v.clone()))
        .collect();
    let bound = design_model::bind_row(&doc, &item.row_id, &values)?;
    let rendered = design_render::render_document(&bound, &item.variant_id, &plan.resources, RenderOptions::default())?;
    thumbnail(&rendered)
}

fn thumbnail(rendered: &design_render::RenderOutput) -> HostResult<Vec<u8>> {
    let fail = |message| error(Code::RenderFailed, message, "Inspect this candidate's native render issues.");
    let (w, h) = (rendered.report.width, rendered.report.height);
    let scale = (MAX_PREVIEW_SIDE as f64 / f64::from(w.max(h))).min(1.0);
    let (tw, th) = (
        (f64::from(w) * scale).round().max(1.0) as i32,
        (f64::from(h) * scale).round().max(1.0) as i32,
    );
    let source_info = ImageInfo::new(
        (w as i32, h as i32),
        ColorType::RGBA8888,
        AlphaType::Unpremul,
        ColorSpace::new_srgb(),
    );
    let image = images::raster_from_data(&source_info, Data::new_copy(&rendered.rgba), w as usize * 4)
        .ok_or_else(|| fail("candidate raster allocation failed"))?;
    let info = ImageInfo::new((tw, th), ColorType::RGBA8888, AlphaType::Premul, ColorSpace::new_srgb());
    let mut surface = surfaces::raster(&info, None, None).ok_or_else(|| fail("thumbnail surface allocation failed"))?;
    surface.canvas().clear(skia_safe::Color::TRANSPARENT);
    surface.canvas().draw_image_rect_with_sampling_options(
        &image,
        None,
        Rect::from_wh(tw as f32, th as f32),
        SamplingOptions::new(FilterMode::Linear, MipmapMode::Linear),
        &Paint::default(),
    );
    let image = surface.image_snapshot();
    let pixels = image.peek_pixels().ok_or_else(|| fail("thumbnail pixel access failed"))?;
    let mut png = Vec::new();
    if !png_encoder::encode(&pixels, &mut png, &png_encoder::Options::default()) {
        return Err(fail("thumbnail PNG encoding failed"));
    }
    if png.len() > MAX_PREVIEW_PNG_BYTES {
        return Err(error(
            Code::ResourceLimit,
            "thumbnail PNG exceeds 2 MiB",
            "Use a smaller candidate artboard.",
        ));
    }
    Ok(png)
}

fn save_snapshot(root: &std::path::Path, id: &str, plan: &Plan, rows: usize) -> HostResult<()> {
    std::fs::create_dir_all(root).map_err(|_| {
        error(
            Code::SaveFailed,
            "cannot create the app-owned batch snapshot directory",
            "Check local disk space.",
        )
    })?;
    grants::DirectoryGrant::new(root.to_owned()).map_err(|i| vec![i])?;
    let path = root.join(id);
    std::fs::create_dir(&path)
        .and_then(|_| std::fs::create_dir(path.join("assets")))
        .and_then(|_| std::fs::create_dir(path.join("fonts")))
        .map_err(|_| {
            error(
                Code::SaveFailed,
                "cannot reserve an immutable batch snapshot",
                "Check local disk space.",
            )
        })?;
    let doc = design_model::load_document(&plan.template)?;
    crate::host::copy_resources(&doc, &plan.resources, &path)?;
    let directory = DirectoryGrant::new(path).map_err(|i| vec![i])?;
    directory
        .write_new("template.json", &plan.template)
        .and_then(|_| directory.write_new("data.csv", &plan.data))
        .map_err(|i| vec![i])?;
    let metadata = SavedSnapshot {
        snapshot_id: id.to_owned(),
        document_title: doc.title,
        revision: doc.revision,
        rows,
        variants: plan.variants.clone(),
        name_pattern: plan.name_pattern.clone(),
        error_policy: plan.policy,
        template_sha256: design_render::resources::sha256_hex(&plan.template),
        data_sha256: design_render::resources::sha256_hex(&plan.data),
        recipes: plan.recipes.clone(),
        status: "ready".into(),
    };
    directory
        .write_new("snapshot.json", &serde_json::to_vec(&metadata).expect("snapshot metadata"))
        .map_err(|i| vec![i])?;
    Ok(())
}

fn snapshot_path(root: &std::path::Path, id: &str) -> HostResult<std::path::PathBuf> {
    if !id.starts_with("plan-") || id.len() != 29 || !id[5..].bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
        return Err(error(
            Code::PathDenied,
            "invalid saved batch snapshot ID",
            "Choose a snapshot from the saved jobs list.",
        ));
    }
    Ok(root.join(id))
}
fn read_snapshot(root: &std::path::Path, id: &str) -> HostResult<(SavedSnapshot, std::path::PathBuf)> {
    let path = snapshot_path(root, id)?;
    let bytes = grants::read_selected(&path.join("snapshot.json"), 20 * 1024).map_err(|i| vec![i])?;
    let saved: SavedSnapshot = serde_json::from_slice(&bytes).map_err(|_| {
        error(
            Code::DataInvalid,
            "saved batch snapshot metadata is invalid",
            "Restore a complete app-owned snapshot.",
        )
    })?;
    if saved.snapshot_id != id
        || saved.rows > job_runner::csv::MAX_ROWS
        || saved.variants.len() > 64
        || saved.name_pattern.len() > 512
        || saved.recipes.is_empty()
        || saved.recipes.len() > 5
    {
        return Err(error(
            Code::DataInvalid,
            "saved batch snapshot metadata exceeds its limits",
            "Restore a complete app-owned snapshot.",
        ));
    }
    Ok((saved, path))
}

#[tauri::command]
pub async fn list_saved_batches(state: State<'_, SharedHost>) -> HostResult<Vec<SavedBatchView>> {
    let shared = state.inner().clone();
    blocking(move || list_saved_batches_plain(&shared)).await
}

pub(crate) fn list_saved_batches_plain(shared: &SharedHost) -> HostResult<Vec<SavedBatchView>> {
    let root = lock(shared)?.workspace.join("batch-snapshots");
    if !root.exists() {
        return Ok(Vec::new());
    }
    grants::DirectoryGrant::new(root.clone()).map_err(|i| vec![i])?;
    let mut snapshots = Vec::new();
    for entry in std::fs::read_dir(&root)
        .map_err(|_| {
            error(
                Code::PathDenied,
                "cannot list app-owned job snapshots",
                "Check local workspace permissions.",
            )
        })?
        .take(1000)
        .flatten()
    {
        if let Some(id) = entry.file_name().to_str()
            && let Ok((saved, _)) = read_snapshot(&root, id)
        {
            snapshots.push(saved.view());
        }
    }
    Ok(snapshots)
}

#[tauri::command]
pub async fn resume_saved_batch(
    app: tauri::AppHandle,
    state: State<'_, SharedHost>,
    batches: State<'_, SharedBatches>,
    snapshot_id: String,
) -> HostResult<Option<JobView>> {
    let shared = state.inner().clone();
    let shared_batches = batches.inner().clone();
    blocking(move || {
        let Some(selected) = app
            .dialog()
            .file()
            .set_title("Choose this job's original output folder to regrant access")
            .blocking_pick_folder()
        else {
            return Ok(None);
        };
        let output_directory = selected.into_path().map_err(|_| {
            error(
                Code::PathDenied,
                "only local output folders are supported",
                "Choose the job's original local folder.",
            )
        })?;
        resume_saved_batch_path(&shared, &shared_batches, snapshot_id, output_directory).map(Some)
    })
    .await
}

pub(crate) fn resume_saved_batch_path(
    shared: &SharedHost,
    shared_batches: &SharedBatches,
    snapshot_id: String,
    output_directory: std::path::PathBuf,
) -> HostResult<JobView> {
    let (root, _lease) = {
        let mut host = lock(shared)?;
        let root = host.workspace.join("batch-snapshots");
        let path = snapshot_path(&root, &snapshot_id)?;
        let lease = host.pin_workspace(shared.clone(), path);
        (root, lease)
    };
    let (saved, path) = read_snapshot(&root, &snapshot_id)?;
    let template = grants::read_selected(&path.join("template.json"), 20 * 1024 * 1024).map_err(|i| vec![i])?;
    let data = grants::read_selected(&path.join("data.csv"), job_runner::csv::MAX_CSV_BYTES as u64).map_err(|i| vec![i])?;
    if design_render::resources::sha256_hex(&template) != saved.template_sha256
        || design_render::resources::sha256_hex(&data) != saved.data_sha256
    {
        return Err(error(
            Code::AssetChecksum,
            "saved batch snapshot checksum mismatch",
            "Restore the original immutable snapshot.",
        ));
    }
    let doc = design_model::load_document(&template)?;
    let table = job_runner::csv::parse(&data).map_err(|i| vec![i])?;
    if doc.revision != saved.revision || table.records.len() != saved.rows {
        return Err(error(
            Code::DataInvalid,
            "saved batch counts or revision differ from the snapshot",
            "Restore the original immutable snapshot.",
        ));
    }
    let resources = ResourceStore::open(&path).map_err(|i| vec![i])?;
    let output = DirectoryGrant::new(output_directory).map_err(|i| vec![i])?;
    let mut plan = Plan {
        template,
        data,
        variants: saved.variants,
        name_pattern: saved.name_pattern,
        resources,
        output,
        policy: saved.error_policy,
        recipes: saved.recipes,
        source_workspace: path.clone(),
        snapshot_path: Some(path),
        items: Vec::new(),
        can_start: false,
    };
    let preflight = job_runner::preflight_export_granted(&plan.request(), plan.output.directory(), &plan.recipes, true)?;
    if plan.policy == job_runner::ErrorPolicy::BlockAll && design_model::has_errors(&preflight.issues) {
        return Err(preflight.issues);
    }
    plan.items = preflight.items;
    plan.can_start = true;
    let mut batches = batch_lock(shared_batches)?;
    start_job(&mut batches, snapshot_id, Arc::new(plan), true)
}

impl Plan {
    fn request(&self) -> job_runner::BatchRequest<'_> {
        job_runner::BatchRequest {
            template: &self.template,
            data: &self.data,
            variants: &self.variants,
            name_pattern: &self.name_pattern,
            manifest_name: "manifest.json",
            store: &self.resources,
            out_dir: &self.output.path,
            options: RenderOptions::default(),
        }
    }
}

fn launch(job: Arc<Job>, resume: bool) {
    std::thread::spawn(move || {
        let progress = |p: job_runner::BatchProgress| {
            if let Ok(mut view) = job.status.lock() {
                view.status = p.status.to_string();
                view.total = p.total;
                view.completed = p.completed;
                view.current_row_id = p.current_row_id;
                view.current_variant_id = p.current_variant_id;
                view.issues = p.issues;
            }
        };
        let request = job.plan.request();
        let result = job_runner::run_export_batch_granted(
            &request,
            job.plan.output.directory(),
            &job.cancel,
            progress,
            job.plan.policy,
            resume,
            &job.plan.recipes,
        );
        if let Ok(mut view) = job.status.lock() {
            match result {
                Ok(manifest) => {
                    view.status = manifest.status.to_string();
                    view.total = manifest.total;
                    view.completed = manifest.items.len();
                    view.issues = manifest.issues;
                }
                Err(issues) => {
                    view.status = if job.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    view.issues = issues;
                }
            }
            view.current_row_id = None;
            view.current_variant_id = None;
            if let Some(path) = &job.plan.snapshot_path
                && let Err(mut issues) = mark_snapshot(path, &view.status)
            {
                view.issues.append(&mut issues);
            }
        }
    });
}

#[tauri::command]
pub fn start_batch(batches: State<'_, SharedBatches>, plan_id: String) -> HostResult<JobView> {
    start_batch_plain(batches.inner(), plan_id)
}

pub(crate) fn start_batch_plain(batches: &SharedBatches, plan_id: String) -> HostResult<JobView> {
    let mut batches = batch_lock(batches)?;
    if let Some(id) = batches.started.get(&plan_id)
        && let Some(job) = batches.jobs.get(id)
    {
        return job
            .status
            .lock()
            .map(|v| v.clone())
            .map_err(|_| error(Code::RenderFailed, "batch progress is unavailable", "Inspect the output manifest."));
    }
    let plan = batches
        .plans
        .get(&plan_id)
        .cloned()
        .ok_or_else(|| error(Code::PathDenied, "unknown batch preflight plan", "Run all-row preflight first."))?;
    start_job(&mut batches, plan_id, plan, false)
}

fn start_job(batches: &mut BatchHost, plan_id: String, plan: Arc<Plan>, resume: bool) -> HostResult<JobView> {
    if !plan.can_start {
        return Err(error(
            Code::EditForbidden,
            "this inspection plan contains blocking errors",
            "Fix the errors and run batch inspection again.",
        ));
    }
    if batches
        .jobs
        .values()
        .filter(|j| {
            j.status
                .lock()
                .is_ok_and(|v| matches!(v.status.as_str(), "queued" | "ready" | "running" | "preflighting" | "cancelling"))
        })
        .count()
        >= 2
    {
        return Err(error(
            Code::ResourceLimit,
            "two batch workers are already active",
            "Wait for a worker to finish or cancel one.",
        ));
    }
    let id = opaque_id("job");
    let total = job_runner::csv::parse(&plan.data).map_err(|i| vec![i])?.records.len() * plan.variants.len() * plan.recipes.len();
    let view = JobView {
        job_id: id.clone(),
        status: "queued".into(),
        total,
        completed: 0,
        current_row_id: None,
        current_variant_id: None,
        issues: Vec::new(),
    };
    let job = Arc::new(Job {
        plan,
        cancel: AtomicBool::new(false),
        status: Mutex::new(view.clone()),
    });
    batches.started.insert(plan_id, id.clone());
    batches.jobs.insert(id, job.clone());
    launch(job, resume);
    Ok(view)
}

#[tauri::command]
pub fn batch_status(batches: State<'_, SharedBatches>, job_id: String) -> HostResult<JobView> {
    batch_status_plain(batches.inner(), job_id)
}

pub(crate) fn batch_status_plain(batches: &SharedBatches, job_id: String) -> HostResult<JobView> {
    let batches = batch_lock(batches)?;
    let job = batches
        .jobs
        .get(&job_id)
        .ok_or_else(|| error(Code::PathDenied, "unknown batch job", "Start a preflighted batch first."))?;
    job.status
        .lock()
        .map(|v| v.clone())
        .map_err(|_| error(Code::RenderFailed, "batch progress is unavailable", "Inspect the output manifest."))
}

#[tauri::command]
pub fn cancel_batch(batches: State<'_, SharedBatches>, job_id: String) -> HostResult<JobView> {
    cancel_batch_plain(batches.inner(), job_id)
}

pub(crate) fn cancel_batch_plain(batches: &SharedBatches, job_id: String) -> HostResult<JobView> {
    let batches = batch_lock(batches)?;
    let job = batches
        .jobs
        .get(&job_id)
        .ok_or_else(|| error(Code::PathDenied, "unknown batch job", "Start a batch first."))?;
    job.cancel.store(true, Ordering::Release);
    let mut view = job
        .status
        .lock()
        .map_err(|_| error(Code::RenderFailed, "batch progress is unavailable", "Inspect the output manifest."))?;
    if matches!(view.status.as_str(), "queued" | "ready" | "running" | "preflighting") {
        view.status = "cancelling".into();
    }
    Ok(view.clone())
}

#[tauri::command]
pub fn resume_batch(batches: State<'_, SharedBatches>, job_id: String) -> HostResult<JobView> {
    resume_batch_plain(batches.inner(), job_id)
}

pub(crate) fn resume_batch_plain(batches: &SharedBatches, job_id: String) -> HostResult<JobView> {
    let batches = batch_lock(batches)?;
    if batches
        .jobs
        .iter()
        .filter(|(id, j)| {
            *id != &job_id
                && j.status
                    .lock()
                    .is_ok_and(|v| matches!(v.status.as_str(), "queued" | "ready" | "running" | "preflighting" | "cancelling"))
        })
        .count()
        >= 2
    {
        return Err(error(
            Code::ResourceLimit,
            "two batch workers are already active",
            "Wait for a worker to finish.",
        ));
    }
    let job = batches
        .jobs
        .get(&job_id)
        .ok_or_else(|| error(Code::PathDenied, "unknown batch job", "Start a batch first."))?;
    let mut view = job
        .status
        .lock()
        .map_err(|_| error(Code::RenderFailed, "batch progress is unavailable", "Inspect the output manifest."))?;
    if !matches!(view.status.as_str(), "cancelled" | "failed" | "interrupted") {
        return Err(error(
            Code::EditForbidden,
            "this job cannot be resumed in its current state",
            "Wait for cancellation or failure to finish.",
        ));
    }
    job.cancel.store(false, Ordering::Release);
    view.status = "queued".into();
    let result = view.clone();
    drop(view);
    launch(job.clone(), true);
    Ok(result)
}

/// Keep the app-owned workspace lock alive until cancelled native workers have
/// reached a terminal status, even after the synchronous FFI facade is closed.
pub(crate) fn shutdown_plain(shared: &SharedHost, batches: &SharedBatches) {
    let shared = shared.clone();
    let batches = batches.clone();
    {
        let state = batches.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        for job in state.jobs.values() {
            job.cancel.store(true, Ordering::Release);
        }
    }
    std::thread::spawn(move || {
        let _host_guard = shared;
        loop {
            let active = {
                let state = batches.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                state.jobs.values().any(|job| {
                    matches!(
                        job.status.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).status.as_str(),
                        "queued" | "ready" | "running" | "preflighting" | "cancelling"
                    )
                })
            };
            if !active {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffi_shutdown_keeps_workspace_locked_until_worker_terminal() {
        let root = std::env::temp_dir().join(opaque_id("shutdown-test"));
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let workspace = root.join("workspace");
        let output = root.join("output");
        std::fs::create_dir_all(&output).unwrap();
        let fixtures =
            std::fs::canonicalize(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/public-safe")).unwrap();
        let shared = Arc::new(Mutex::new(crate::host::Host::new(workspace.clone(), fixtures.clone()).unwrap()));
        let plan = Plan {
            template: vec![],
            data: vec![],
            variants: vec![],
            name_pattern: "{rowId}_{variantId}.png".into(),
            resources: ResourceStore::open(&fixtures.join("resources")).unwrap(),
            output: DirectoryGrant::new(output).unwrap(),
            policy: job_runner::ErrorPolicy::BlockAll,
            recipes: default_recipes(),
            source_workspace: workspace.clone(),
            snapshot_path: None,
            items: vec![],
            can_start: true,
        };
        let job = Arc::new(Job {
            plan: Arc::new(plan),
            cancel: AtomicBool::new(false),
            status: Mutex::new(JobView {
                job_id: "job-test".into(),
                status: "running".into(),
                total: 1,
                completed: 0,
                current_row_id: None,
                current_variant_id: None,
                issues: vec![],
            }),
        });
        let mut state = BatchHost::default();
        state.jobs.insert("job-test".into(), job.clone());
        let batches = Arc::new(Mutex::new(state));
        shutdown_plain(&shared, &batches);
        assert!(job.cancel.load(Ordering::Acquire));
        drop(shared);
        drop(batches);
        assert!(crate::host::Host::new(workspace.clone(), fixtures.clone()).is_err());
        // Represent the native worker reaching its cancellation boundary.
        job.status.lock().unwrap().status = "cancelled".into();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let Ok(host) = crate::host::Host::new(workspace.clone(), fixtures.clone()) {
                drop(host);
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "closed FFI facade retained a terminal worker's workspace lock"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        drop(job);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn json_and_csv_map_to_identical_frozen_bytes_without_value_coercion() {
        let csv = parse_dataset(b"price,note,id\n0009,\"=SUM(1,2)\",001\n", false).unwrap();
        let json = parse_dataset(br#"[{"id":"001","price":"0009","note":"=SUM(1,2)"}]"#, true).unwrap();
        let mapping = BTreeMap::from([
            ("id".into(), "rowId".into()),
            ("price".into(), "price-slot".into()),
            ("note".into(), "headline-slot".into()),
        ]);
        let mapped = mapped_csv(&json, &mapping).unwrap();
        assert_eq!(mapped, mapped_csv(&csv, &mapping).unwrap());
        let decoded = job_runner::csv::parse(&mapped).unwrap();
        assert_eq!(decoded.records[0].1, ["001", "=SUM(1,2)", "0009"]);
        let reordered = parse_dataset(br#"[{"note":"=SUM(1,2)","price":"0009","id":"001"}]"#, false).unwrap();
        assert_eq!(mapped, mapped_csv(&reordered, &mapping).unwrap());
        assert_eq!(
            design_render::resources::sha256_hex(&mapped),
            design_render::resources::sha256_hex(&mapped_csv(&csv, &mapping).unwrap())
        );
        let line_break = parse_dataset(br#"[{"id":"001","price":"0009","note":"line1\nline2"}]"#, true).unwrap();
        assert_eq!(
            job_runner::csv::parse(&mapped_csv(&line_break, &mapping).unwrap()).unwrap().records[0].1[1],
            "line1\nline2"
        );
        let carriage_return = parse_dataset(br#"[{"id":"001","price":"0009","note":"line1\r\nline2"}]"#, true).unwrap();
        assert_eq!(mapped_csv(&carriage_return, &mapping).unwrap_err()[0].code, Code::DataInvalid);
    }
    #[test]
    fn json_rejects_duplicate_columns_nonstrings_shape_and_empty_data() {
        for input in [
            r#"[{"id":"001","id":"002"}]"#,
            r#"[{"id":"001","\u0069d":"002"}]"#,
            r#"[{"id":1}]"#,
            r#"[{"id":null}]"#,
            r#"[{"id":true}]"#,
            r#"[{"id":[]}]"#,
            r#"[{"id":{}}]"#,
            r#"{"id":"001"}"#,
            "42",
            "null",
            "[[]]",
            "[]",
            "[{}]",
            r#"[{"":"001"}]"#,
            r#"[{"id":"001"},{"id":"002","extra":"x"}]"#,
            r#"[{"id":"001","extra":"x"},{"id":"002"}]"#,
            r#"[{"id":"001"}] trailing"#,
            "",
        ] {
            let issues = parse_dataset(input.as_bytes(), true).unwrap_err();
            assert_eq!(issues[0].code, Code::DataInvalid, "{input}");
        }
        assert!(parse_dataset(b"id\n", false).is_err());
        assert!(parse_dataset(&[0xff], true).is_err());
        assert!(parse_dataset(b"id,value\n001\n", false).is_err());
    }
    #[test]
    fn json_limits_are_checked_before_unbounded_row_allocation() {
        let row = r#"{"id":"001"}"#;
        let maximum = format!("[{}]", vec![row; job_runner::csv::MAX_ROWS].join(","));
        assert_eq!(
            parse_dataset(maximum.as_bytes(), true).unwrap().records.len(),
            job_runner::csv::MAX_ROWS
        );
        let excessive = format!("[{}]", vec![row; job_runner::csv::MAX_ROWS + 1].join(","));
        assert_eq!(parse_dataset(excessive.as_bytes(), true).unwrap_err()[0].code, Code::ResourceLimit);
        assert_eq!(
            parse_dataset(&vec![b' '; job_runner::csv::MAX_CSV_BYTES + 1], true).unwrap_err()[0].code,
            Code::ResourceLimit
        );
    }
    #[test]
    fn explicit_mapping_preserves_zeroes_and_rejects_duplicate_targets() {
        let table = job_runner::csv::parse(b"id,price,unused\n001,0009,ignored\n").unwrap();
        let mapped = mapped_csv(
            &table,
            &BTreeMap::from([("id".into(), "rowId".into()), ("price".into(), "price-slot".into())]),
        )
        .unwrap();
        let parsed = job_runner::csv::parse(&mapped).unwrap();
        assert_eq!(parsed.records[0].1, vec!["001", "0009"]);
        assert!(
            mapped_csv(
                &table,
                &BTreeMap::from([("id".into(), "rowId".into()), ("price".into(), "rowId".into())])
            )
            .is_err()
        );
    }
    #[test]
    fn saved_snapshot_reopens_with_identical_native_output() {
        let root = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(opaque_id("batch-snapshot-test"));
        let out = root.join("output");
        std::fs::create_dir_all(&out).unwrap();
        let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/public-safe");
        let template = std::fs::read(fixtures.join("documents/basic.json")).unwrap();
        let doc = design_model::load_document(&template).unwrap();
        let mut plan = Plan {
            template,
            data: b"rowId,headline-slot,photo-slot\n001,Quality,photo\n".to_vec(),
            variants: vec!["square".into()],
            name_pattern: "{rowId}-{variantId}.png".into(),
            resources: ResourceStore::open(&fixtures.join("resources")).unwrap(),
            output: DirectoryGrant::new(out).unwrap(),
            policy: job_runner::ErrorPolicy::BlockAll,
            recipes: default_recipes(),
            source_workspace: fixtures.join("resources"),
            snapshot_path: None,
            items: Vec::new(),
            can_start: true,
        };
        let preflight = job_runner::preflight_export_granted(&plan.request(), plan.output.directory(), &plan.recipes, false).unwrap();
        assert_eq!(preflight.items.len(), 1);
        plan.items = preflight.items;
        let preview = render_batch_thumbnail(&plan, 0).unwrap();
        assert_eq!(&preview[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(preview[16..20].try_into().unwrap()), 512);
        assert_eq!(u32::from_be_bytes(preview[20..24].try_into().unwrap()), 512);
        assert!(preview.len() <= MAX_PREVIEW_PNG_BYTES);
        assert_eq!(render_batch_thumbnail(&plan, 0).unwrap(), preview);
        assert!(render_batch_thumbnail(&plan, usize::MAX).is_err());
        let mut blocked = plan.items[0].clone();
        blocked
            .issues
            .push(Issue::error(Code::DataInvalid, "required field missing", "Fill the field."));
        plan.items.push(blocked.clone());
        assert_eq!(render_batch_thumbnail(&plan, 1).unwrap_err(), blocked.issues);
        plan.items.pop();
        // Only frozen template/data are read by candidate rendering: mutating
        // another authoring document cannot change the inspected thumbnail.
        let mut authoring_doc = doc.clone();
        authoring_doc.title = "A later edit".into();
        authoring_doc.revision += 1;
        assert_eq!(render_batch_thumbnail(&plan, 0).unwrap(), preview);
        let id = opaque_id("plan");
        save_snapshot(&root.join("saved"), &id, &plan, 1).unwrap();
        let (saved, path) = read_snapshot(&root.join("saved"), &id).unwrap();
        assert_eq!(saved.rows, 1);
        assert_eq!(saved.template_sha256, design_render::resources::sha256_hex(&plan.template));
        assert_eq!(std::fs::read(path.join("data.csv")).unwrap(), plan.data);
        let reopened = ResourceStore::open(&path).unwrap();
        let before = design_render::render_document(&doc, "square", &plan.resources, RenderOptions::default()).unwrap();
        let after = design_render::render_document(&doc, "square", &reopened, RenderOptions::default()).unwrap();
        assert_eq!(before.png, after.png);
        assert!(snapshot_path(&root, "../escape").is_err());
        assert!(!completed_snapshot(&root.join("saved"), &id));
        mark_snapshot(&path, "cancelled").unwrap();
        assert!(!completed_snapshot(&root.join("saved"), &id));
        plan.snapshot_path = Some(path);
        let mut batches = BatchHost::default();
        plan.can_start = false;
        let plan = Arc::new(plan);
        assert!(start_job(&mut batches, id.clone(), plan.clone(), false).is_err());
        let mut plan = Arc::try_unwrap(plan).ok().unwrap();
        plan.can_start = true;
        let view = start_job(&mut batches, id.clone(), Arc::new(plan), false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let status = batches.jobs.get(&view.job_id).unwrap().status.lock().unwrap().status.clone();
            if status == "completed" {
                break;
            }
            assert!(!matches!(status.as_str(), "failed" | "cancelled"));
            assert!(std::time::Instant::now() < deadline, "native fixture batch did not finish");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(completed_snapshot(&root.join("saved"), &id));
        assert!(batches.protected_paths().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
