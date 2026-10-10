use design_export::{ExportFormat, ExportRecipe};
use design_model::{Code, load_document};
use design_render::resources::{ResourceStore, sha256_hex};
use job_runner::{BatchRequest, ErrorPolicy, preflight, resume_batch_controlled, run_batch_controlled, run_batch_granted};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

static NONCE: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let dir = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "siliq-controlled-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}
fn store() -> ResourceStore {
    ResourceStore::open(&root().join("fixtures/public-safe/resources")).unwrap()
}
fn simple_template() -> Vec<u8> {
    let mut doc = load_document(&fs::read(root().join("fixtures/public-safe/documents/basic.json")).unwrap()).unwrap();
    doc.nodes.retain(|n| n.id() == "root" || n.id() == "background");
    if let design_model::Node::Group { child_ids, .. } = &mut doc.nodes[0] {
        *child_ids = vec!["background".into()];
    }
    for n in &mut doc.nodes {
        n.common_mut().bounds.width = 64.;
        n.common_mut().bounds.height = 64.;
    }
    doc.base_artboard.width = 64.;
    doc.base_artboard.height = 64.;
    doc.fonts.clear();
    doc.assets.clear();
    doc.slots.clear();
    doc.variants.retain(|v| v.id == "square");
    doc.variants[0].width = 64.;
    doc.variants[0].height = 64.;
    serde_json::to_vec(&doc).unwrap()
}
fn req<'a>(template: &'a [u8], data: &'a [u8], variants: &'a [String], store: &'a ResourceStore, path: &'a Path) -> BatchRequest<'a> {
    BatchRequest {
        template,
        data,
        variants,
        name_pattern: "{rowId}_{variantId}.png",
        manifest_name: "manifest.json",
        store,
        out_dir: path,
        options: Default::default(),
    }
}
fn names(path: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn preflight_lists_all_outputs_without_writing_files() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\nc\n", &variants, &store, &temp.0);
    let result = preflight(&req).unwrap();
    assert_eq!((result.rows, result.total, result.items.len()), (3, 3, 3));
    assert_eq!(result.items[1].file, "b_square.png");
    assert!(result.issues.is_empty());
    assert!(names(&temp.0).is_empty());
}

#[test]
fn cancellation_before_first_native_item_still_has_a_resumable_snapshot() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    let cancelled = AtomicBool::new(true);
    let result = run_batch_controlled(&req, &cancelled, |_| {}).unwrap();
    assert_eq!(result.status, "cancelled");
    assert!(result.items.is_empty());
    cancelled.store(false, Ordering::Release);
    assert_eq!(resume_batch_controlled(&req, &cancelled, |_| {}).unwrap().items.len(), 2);
}

#[test]
fn cancellation_retains_verified_work_then_resume_finishes_exact_snapshot() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\nc\n", &variants, &store, &temp.0);
    let cancel = AtomicBool::new(false);
    let result = run_batch_controlled(&req, &cancel, |p| {
        if p.status == "running" && p.completed == 1 {
            cancel.store(true, Ordering::Release);
        }
    })
    .unwrap();
    assert_eq!(result.status, "cancelled");
    assert_eq!(result.items.len(), 1);
    assert!(!temp.0.join("a_square.png").exists());
    assert!(!temp.0.join("manifest.json").exists());
    cancel.store(false, Ordering::Release);
    let completed = resume_batch_controlled(&req, &cancel, |_| {}).unwrap();
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.items.len(), 3);
    assert_eq!(
        names(&temp.0),
        vec!["a_square.png", "b_square.png", "c_square.png", "manifest.json"]
    );
    for item in completed.items {
        assert_eq!(sha256_hex(&fs::read(temp.0.join(item.file)).unwrap()), item.sha256);
    }
}

#[test]
fn crash_after_prepare_or_publication_resumes_only_matching_bytes() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\nc\n", &variants, &store, &temp.0);
    let cancel = AtomicBool::new(false);
    let cancelled = run_batch_controlled(&req, &cancel, |p| {
        if p.status == "running" && p.completed == 3 {
            cancel.store(true, Ordering::Release);
        }
    })
    .unwrap();
    assert_eq!(cancelled.items.len(), 3);
    let stage = temp.0.join(format!(".siliq-job-{}", cancelled.input_digest));
    // Simulate termination after a no-clobber publication and before the
    // following journal update. The complete prepared record already exists.
    fs::hard_link(stage.join("a_square.png"), temp.0.join("a_square.png")).unwrap();
    fs::remove_file(stage.join("a_square.png")).unwrap();
    cancel.store(false, Ordering::Release);
    let result = resume_batch_controlled(&req, &cancel, |_| {}).unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(names(&temp.0).len(), 4);
}

#[test]
fn terminated_worker_leaves_an_atomic_journal_that_can_resume() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\nc\n", &variants, &store, &temp.0);
    let cancel = AtomicBool::new(false);
    let aborted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_batch_controlled(&req, &cancel, |p| {
            if p.status == "running" && p.completed == 1 {
                panic!("simulated worker termination after durable preparation");
            }
        })
    }));
    assert!(aborted.is_err());
    assert!(!temp.0.join("manifest.json").exists());
    assert_eq!(resume_batch_controlled(&req, &cancel, |_| {}).unwrap().items.len(), 3);
}

#[test]
fn output_creation_race_keeps_the_competing_file_and_the_job_journal() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    let result = run_batch_controlled(&req, &AtomicBool::new(false), |p| {
        if p.status == "running" && p.completed == 2 {
            fs::write(temp.0.join("a_square.png"), b"competing file").unwrap();
        }
    });
    assert!(result.unwrap_err().iter().any(|i| i.code == Code::OutputCollision));
    assert_eq!(fs::read(temp.0.join("a_square.png")).unwrap(), b"competing file");
    assert!(!temp.0.join("manifest.json").exists());
    assert!(names(&temp.0).iter().any(|n| n.starts_with(".siliq-job-")));
}

#[test]
fn truncated_journal_is_an_error_and_preserves_prepared_files() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    let cancel = AtomicBool::new(false);
    let result = run_batch_controlled(&req, &cancel, |p| {
        if p.status == "running" && p.completed == 1 {
            cancel.store(true, Ordering::Release);
        }
    })
    .unwrap();
    let stage = temp.0.join(format!(".siliq-job-{}", result.input_digest));
    let prepared = fs::read(stage.join("a_square.png")).unwrap();
    fs::write(stage.join("journal.json"), b"{\"version\":1,").unwrap();
    cancel.store(false, Ordering::Release);
    assert!(resume_batch_controlled(&req, &cancel, |_| {}).is_err());
    assert_eq!(fs::read(stage.join("a_square.png")).unwrap(), prepared);
}

#[test]
fn completed_resume_verifies_every_output_and_never_overwrites_corruption() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    let cancel = AtomicBool::new(false);
    let completed = run_batch_controlled(&req, &cancel, |_| {}).unwrap();
    let repeated = resume_batch_controlled(&req, &cancel, |_| {}).unwrap();
    assert_eq!(completed.input_digest, repeated.input_digest);
    fs::write(temp.0.join("a_square.png"), b"keep corrupted file").unwrap();
    let err = resume_batch_controlled(&req, &cancel, |_| {}).unwrap_err();
    assert!(err.iter().any(|i| i.code == Code::AssetChecksum));
    assert_eq!(fs::read(temp.0.join("a_square.png")).unwrap(), b"keep corrupted file");
}

#[test]
fn changed_data_or_recipe_cannot_resume_an_existing_job() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let cancel = AtomicBool::new(false);
    let original = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    run_batch_controlled(&original, &cancel, |_| {}).unwrap();
    let changed = req(&template, b"rowId\na\nc\n", &variants, &store, &temp.0);
    assert!(
        resume_batch_controlled(&changed, &cancel, |_| {})
            .unwrap_err()
            .iter()
            .any(|i| i.code == Code::DataInvalid)
    );
    let mut recipe = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    recipe.name_pattern = "other_{rowId}_{variantId}.png";
    assert!(resume_batch_controlled(&recipe, &cancel, |_| {}).is_err());
    assert!(!temp.0.join("c_square.png").exists());
}

#[test]
fn partial_outputs_require_explicit_valid_only_and_report_failures() {
    let temp = Temp::new();
    let store = store();
    let variants = vec!["square".into()];
    let mut doc = load_document(&fs::read(root().join("fixtures/public-safe/documents/basic.json")).unwrap()).unwrap();
    doc.slots.retain(|s| s.id == "headline-slot");
    doc.slots[0].max_length = Some(1);
    let template = serde_json::to_vec(&doc).unwrap();
    let req = req(
        &template,
        b"rowId,headline-slot\ngood,A\nbad,too long\n",
        &variants,
        &store,
        &temp.0,
    );
    let summary = preflight(&req).unwrap();
    assert_eq!(summary.total, 2);
    assert!(summary.issues.iter().any(|i| i.location.row_id.as_deref() == Some("bad")));
    assert!(run_batch_controlled(&req, &AtomicBool::new(false), |_| {}).is_err());
    assert!(names(&temp.0).is_empty());
    let directory = File::open(&temp.0).unwrap();
    let manifest = run_batch_granted(&req, &directory, &AtomicBool::new(false), |_| {}, ErrorPolicy::ValidOnly, false).unwrap();
    assert_eq!(manifest.status, "completed_with_errors");
    assert_eq!(manifest.total, 2);
    assert_eq!(manifest.items.len(), 1);
    assert!(!manifest.issues.is_empty());
    assert!(temp.0.join("good_square.png").exists());
    assert!(!temp.0.join("bad_square.png").exists());
}

#[test]
fn valid_only_resume_does_not_publish_a_prepared_item_that_now_fails_native_preflight() {
    let temp = Temp::new();
    let original_store = store();
    let variants = vec!["square".into()];
    let mut doc = load_document(&fs::read(root().join("fixtures/public-safe/documents/basic.json")).unwrap()).unwrap();
    doc.slots.retain(|s| s.id == "headline-slot");
    doc.slots[0].max_length = Some(1);
    let template = serde_json::to_vec(&doc).unwrap();
    let cancelled = AtomicBool::new(false);
    let directory = File::open(&temp.0).unwrap();
    let first = req(
        &template,
        b"rowId,headline-slot\ngood,A\nbad,too long\n",
        &variants,
        &original_store,
        &temp.0,
    );
    let job = job_runner::run_batch_granted(
        &first,
        &directory,
        &cancelled,
        |p| {
            if p.status == "running" && p.completed == 1 {
                cancelled.store(true, Ordering::Release);
            }
        },
        ErrorPolicy::ValidOnly,
        false,
    )
    .unwrap();
    assert_eq!(job.status, "cancelled");
    assert_eq!(job.items.len(), 1);
    // The fixed font cannot be resolved after a simulated resource loss.
    let missing = temp.0.join("missing-resources");
    fs::create_dir(&missing).unwrap();
    fs::create_dir(missing.join("assets")).unwrap();
    for asset in &doc.assets {
        let file = format!("{}.{}", asset.sha256.as_deref().unwrap(), asset.mime_hint.extension());
        fs::copy(
            root().join("fixtures/public-safe/resources/assets").join(&file),
            missing.join("assets").join(file),
        )
        .unwrap();
    }
    let missing_store = ResourceStore::open(&missing).unwrap();
    let resumed = req(&template, first.data, &variants, &missing_store, &temp.0);
    cancelled.store(false, Ordering::Release);
    let result = job_runner::run_batch_granted(&resumed, &directory, &cancelled, |_| {}, ErrorPolicy::ValidOnly, true).unwrap();
    assert_eq!(result.status, "completed_with_errors");
    assert!(result.items.is_empty());
    assert!(result.issues.iter().any(|i| i.code == Code::FontMissing));
    assert!(!temp.0.join("good_square.png").exists());
}

#[test]
fn grants_survive_directory_rename_without_writes_to_a_replacement_path() {
    let temp = Temp::new();
    let selected = temp.0.join("selected");
    fs::create_dir(&selected).unwrap();
    let directory = File::open(&selected).unwrap();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\n", &variants, &store, &selected);
    let moved = temp.0.join("moved");
    fs::rename(&selected, &moved).unwrap();
    fs::create_dir(&selected).unwrap();
    run_batch_granted(&req, &directory, &AtomicBool::new(false), |_| {}, ErrorPolicy::BlockAll, false).unwrap();
    assert!(moved.join("a_square.png").exists());
    assert!(names(&selected).is_empty());
}

#[test]
fn case_alias_output_names_are_rejected_portably() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\nCase\ncase\n", &variants, &store, &temp.0);
    assert!(preflight(&req).unwrap_err().iter().any(|i| i.code == Code::OutputCollision));
    assert!(names(&temp.0).is_empty());
}

fn all_recipes() -> Vec<ExportRecipe> {
    vec![
        ExportRecipe::default(),
        ExportRecipe {
            format: ExportFormat::Jpeg,
            quality: Some(85),
            jpeg_background: Some("#ffffff".into()),
            ..Default::default()
        },
        ExportRecipe {
            format: ExportFormat::Webp,
            ..Default::default()
        },
        ExportRecipe {
            format: ExportFormat::Svg,
            allow_rasterization: true,
            ..Default::default()
        },
        ExportRecipe {
            format: ExportFormat::Pdf,
            allow_rasterization: true,
            pdf_dpi: 144,
            ..Default::default()
        },
    ]
}

#[test]
fn rows_variants_and_five_explicit_formats_produce_real_files_and_conversion_reports() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let mut req = req(&template, b"rowId\na\nb\n", &variants, &store, &temp.0);
    req.name_pattern = "{rowId}_{variantId}.{format}";
    let dir = File::open(&temp.0).unwrap();
    let recipes = all_recipes();
    let plan = job_runner::preflight_export_granted(&req, &dir, &recipes, false).unwrap();
    assert_eq!(plan.total, 10);
    assert!(plan.issues.is_empty());
    let result =
        job_runner::run_export_batch_granted(&req, &dir, &AtomicBool::new(false), |_| {}, ErrorPolicy::BlockAll, false, &recipes).unwrap();
    assert_eq!(result.total, 10);
    assert_eq!(result.items.len(), 10);
    assert_eq!(names(&temp.0).len(), 11);
    for item in &result.items {
        let bytes = fs::read(temp.0.join(&item.file)).unwrap();
        assert_eq!(sha256_hex(&bytes), item.sha256);
        match item.format {
            ExportFormat::Png => assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
            ExportFormat::Jpeg => assert!(bytes.starts_with(&[0xff, 0xd8])),
            ExportFormat::Webp => {
                assert_eq!(&bytes[..4], b"RIFF");
                assert_eq!(&bytes[8..12], b"WEBP");
            }
            ExportFormat::Svg => {
                assert!(bytes.starts_with(b"<svg"));
                assert_eq!(item.export_report["rasterized"], true);
            }
            ExportFormat::Pdf => {
                assert!(bytes.starts_with(b"%PDF-"));
                assert_eq!(item.export_report["rasterizationDpi"], 144);
            }
        }
    }
    let resumed =
        job_runner::run_export_batch_granted(&req, &dir, &AtomicBool::new(false), |_| {}, ErrorPolicy::BlockAll, true, &recipes).unwrap();
    assert_eq!(resumed.input_digest, result.input_digest);
    assert_eq!(resumed.items.len(), 10);
}

#[test]
fn export_settings_are_fixed_and_unsafe_conversions_block_the_entire_batch() {
    let temp = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\n", &variants, &store, &temp.0);
    let dir = File::open(&temp.0).unwrap();
    let missing_background = vec![
        ExportRecipe::default(),
        ExportRecipe {
            format: ExportFormat::Jpeg,
            ..Default::default()
        },
    ];
    let plan = job_runner::preflight_export_granted(&req, &dir, &missing_background, false).unwrap();
    assert!(
        plan.issues
            .iter()
            .any(|i| i.code == Code::ExportUnsupported && i.location.export_format.as_deref() == Some("jpeg"))
    );
    assert!(
        job_runner::run_export_batch_granted(
            &req,
            &dir,
            &AtomicBool::new(false),
            |_| {},
            ErrorPolicy::BlockAll,
            false,
            &missing_background
        )
        .is_err()
    );
    assert!(names(&temp.0).is_empty());
    let recipes = all_recipes();
    let mut changed = recipes.clone();
    changed[1].quality = Some(60);
    assert_ne!(
        job_runner::export_batch_input_digest(&req, ErrorPolicy::BlockAll, &recipes),
        job_runner::export_batch_input_digest(&req, ErrorPolicy::BlockAll, &changed)
    );
}

#[cfg(unix)]
#[test]
fn symlink_output_directory_and_resume_target_are_refused() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let outside = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let link = temp.0.join("link");
    symlink(&outside.0, &link).unwrap();
    let unsafe_req = req(&template, b"rowId\na\n", &variants, &store, &link);
    assert!(
        run_batch_controlled(&unsafe_req, &AtomicBool::new(false), |_| {})
            .unwrap_err()
            .iter()
            .any(|i| i.code == Code::PathDenied)
    );
    let req = req(&template, b"rowId\na\n", &variants, &store, &temp.0);
    run_batch_controlled(&req, &AtomicBool::new(false), |_| {}).unwrap();
    let output = temp.0.join("a_square.png");
    fs::remove_file(&output).unwrap();
    fs::write(outside.0.join("protected"), b"keep").unwrap();
    symlink(outside.0.join("protected"), &output).unwrap();
    assert!(resume_batch_controlled(&req, &AtomicBool::new(false), |_| {}).is_err());
    assert_eq!(fs::read(outside.0.join("protected")).unwrap(), b"keep");
}

#[cfg(unix)]
#[test]
fn foreign_hardlink_resume_target_is_refused_even_when_its_bytes_match() {
    let temp = Temp::new();
    let outside = Temp::new();
    let template = simple_template();
    let store = store();
    let variants = vec!["square".into()];
    let req = req(&template, b"rowId\na\n", &variants, &store, &temp.0);
    let cancelled = AtomicBool::new(false);
    run_batch_controlled(&req, &cancelled, |_| {}).unwrap();
    fs::hard_link(temp.0.join("a_square.png"), outside.0.join("protected.png")).unwrap();
    let original = fs::read(outside.0.join("protected.png")).unwrap();
    assert!(
        resume_batch_controlled(&req, &cancelled, |_| {})
            .unwrap_err()
            .iter()
            .any(|i| i.code == Code::PathDenied)
    );
    assert_eq!(fs::read(outside.0.join("protected.png")).unwrap(), original);
}
