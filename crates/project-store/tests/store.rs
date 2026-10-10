use design_model::{Code, DesignDocument, PackagePolicy, Severity, load_document};
use design_render::resources::{ResourceStore, sha256_hex};
use project_store::{load_project, recover_project, save_project, write_recovery};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

static NONCE: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        // On macOS /var and /tmp are symlinks; use the resolved app/test root.
        let root = std::env::temp_dir().canonicalize().unwrap();
        let path = root.join(format!(
            "siliq-package-test-{}-{}",
            std::process::id(),
            NONCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn project(&self) -> PathBuf {
        self.0.join("design.siliqcg")
    }
    fn workspace(&self) -> PathBuf {
        let p = self.0.join("workspace");
        fs::create_dir_all(&p).unwrap();
        p
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
fn resources() -> ResourceStore {
    ResourceStore::open(&root().join("fixtures/public-safe/resources")).unwrap()
}
fn fixture() -> DesignDocument {
    load_document(&fs::read(root().join("fixtures/public-safe/documents/basic.json")).unwrap()).unwrap()
}
fn simple() -> DesignDocument {
    let mut doc = fixture();
    doc.nodes.retain(|node| node.id() == "root" || node.id() == "background");
    if let design_model::Node::Group { child_ids, .. } = &mut doc.nodes[0] {
        *child_ids = vec!["background".into()];
    }
    doc.fonts.clear();
    doc.assets.clear();
    doc.slots.clear();
    doc.variants.retain(|variant| variant.id == "square");
    doc
}
fn archive_entries(path: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(File::open(path).unwrap()).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut entry = zip.by_index(index).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}
fn write_zip(path: &Path, entries: Vec<(String, Vec<u8>)>) {
    let mut writer = ZipWriter::new(File::create(path).unwrap());
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .unix_permissions(0o600);
    for (name, bytes) in entries {
        writer.start_file(name, options).unwrap();
        writer.write_all(&bytes).unwrap();
    }
    writer.finish().unwrap();
}
fn saved(temp: &Temp) {
    save_project(&temp.project(), &simple(), &resources(), false).unwrap();
}

#[test]
fn round_trip_preserves_native_document_assets_and_confirmed_fonts() {
    let temp = Temp::new();
    let doc = fixture();
    let receipt = save_project(&temp.project(), &doc, &resources(), false).unwrap();
    assert_eq!(receipt.checksum, sha256_hex(&fs::read(temp.project()).unwrap()));
    let loaded = load_project(&temp.project(), &temp.workspace()).unwrap();
    assert_eq!(loaded.document, doc);
    assert!(loaded.issues.is_empty());
    for asset in &doc.assets {
        let (bytes, _) = loaded
            .resources
            .read(
                "assets",
                asset.sha256.as_ref().unwrap(),
                &[asset.mime_hint.extension()],
                100 * 1024 * 1024,
            )
            .unwrap();
        assert_eq!(sha256_hex(&bytes), *asset.sha256.as_ref().unwrap());
    }
    let font = &doc.fonts[0];
    assert!(
        loaded
            .resources
            .read("fonts", font.sha256.as_ref().unwrap(), &["otf"], 64 * 1024 * 1024)
            .is_ok()
    );
    assert!(loaded.workspace.starts_with(temp.workspace()));
}

#[test]
fn reference_only_font_bytes_never_travel() {
    let temp = Temp::new();
    let mut doc = fixture();
    doc.fonts[0].package_policy = PackagePolicy::ReferenceOnly;
    save_project(&temp.project(), &doc, &resources(), false).unwrap();
    assert!(
        !archive_entries(&temp.project())
            .iter()
            .any(|(name, _)| name.starts_with("fonts/") && name != "fonts/manifest.json")
    );
    let loaded = load_project(&temp.project(), &temp.workspace()).unwrap();
    assert_eq!(loaded.document, doc);
    assert!(
        loaded
            .issues
            .iter()
            .any(|i| i.code == Code::FontMissing && i.severity == Severity::Error)
    );
}

#[test]
fn overwrite_requires_explicit_intent_and_failed_save_preserves_original() {
    let temp = Temp::new();
    saved(&temp);
    let original = fs::read(temp.project()).unwrap();
    let mut changed = simple();
    changed.revision += 1;
    changed.title = "changed".into();
    assert_eq!(
        save_project(&temp.project(), &changed, &resources(), false).unwrap_err().code,
        Code::OutputCollision
    );
    assert_eq!(fs::read(temp.project()).unwrap(), original);
    let mut invalid = changed.clone();
    invalid.base_artboard.width = -1.;
    assert!(save_project(&temp.project(), &invalid, &resources(), true).is_err());
    assert_eq!(fs::read(temp.project()).unwrap(), original);
    save_project(&temp.project(), &changed, &resources(), true).unwrap();
    assert_eq!(load_project(&temp.project(), &temp.workspace()).unwrap().document, changed);
    assert!(
        !fs::read_dir(&temp.0)
            .unwrap()
            .any(|e| e.unwrap().file_name().to_string_lossy().starts_with(".siliq-save"))
    );
}

#[test]
fn deterministic_packages_have_no_clock_dependent_zip_metadata() {
    let temp = Temp::new();
    saved(&temp);
    let second = temp.0.join("second.siliqcg");
    save_project(&second, &simple(), &resources(), false).unwrap();
    assert_eq!(fs::read(temp.project()).unwrap(), fs::read(second).unwrap());
}

#[test]
fn missing_resolved_asset_is_a_save_error() {
    let temp = Temp::new();
    let empty = temp.workspace();
    fs::create_dir(empty.join("assets")).unwrap();
    let empty = ResourceStore::open(&empty).unwrap();
    assert_eq!(
        save_project(&temp.project(), &fixture(), &empty, false).unwrap_err().code,
        Code::AssetMissing
    );
    assert!(!temp.project().exists());
}

#[test]
fn zip_slip_unknown_names_case_alias_and_null_are_rejected_before_extraction() {
    for name in [
        "../outside",
        "/absolute",
        "assets/../design.json",
        "Design.json",
        "assets\\bad.png",
        "fonts/CON.ttf",
        "design.json\0extra",
    ] {
        let temp = Temp::new();
        write_zip(&temp.project(), vec![(name.into(), b"unsafe".to_vec())]);
        let workspace = temp.workspace();
        assert!(load_project(&temp.project(), &workspace).is_err(), "accepted {name:?}");
        assert_eq!(fs::read_dir(workspace).unwrap().count(), 0);
    }
}

#[test]
fn corrupted_checksum_unknown_version_and_required_engine_stop_loading() {
    for change in 0..5 {
        let temp = Temp::new();
        saved(&temp);
        let mut entries = archive_entries(&temp.project());
        if change == 0 {
            let bytes = &mut entries.iter_mut().find(|(n, _)| n == "design.json").unwrap().1;
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["title"] = "corruption".into();
            *bytes = serde_json::to_vec(&value).unwrap();
        } else {
            let bytes = &mut entries.iter_mut().find(|(n, _)| n == "manifest.json").unwrap().1;
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            match change {
                1 => value["formatVersion"] = "999.0.0".into(),
                2 => value["schemaVersion"] = "999.0.0-core".into(),
                3 => value["minimumEngineVersion"] = "999.0.0".into(),
                4 => {
                    let list = value["entries"].as_array_mut().unwrap();
                    list[1] = list[0].clone();
                }
                _ => unreachable!(),
            }
            *bytes = serde_json::to_vec(&value).unwrap();
        }
        write_zip(&temp.project(), entries);
        let workspace = temp.workspace();
        let err = load_project(&temp.project(), &workspace).unwrap_err();
        assert!(matches!(err.code, Code::PackageInvalid | Code::AssetChecksum));
        assert_eq!(fs::read_dir(workspace).unwrap().count(), 0);
    }
}

#[test]
fn embedded_font_without_permission_is_rejected() {
    let temp = Temp::new();
    save_project(&temp.project(), &fixture(), &resources(), false).unwrap();
    let mut entries = archive_entries(&temp.project());
    for (name, bytes) in &mut entries {
        if name == "design.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["fonts"][0]["packagePolicy"] = "reference-only".into();
            *bytes = serde_json::to_vec(&value).unwrap();
        } else if name == "fonts/manifest.json" {
            let mut value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            value["fonts"][0]["font"]["packagePolicy"] = "reference-only".into();
            *bytes = serde_json::to_vec(&value).unwrap();
        }
    }
    let mut manifest: serde_json::Value = serde_json::from_slice(&entries.iter().find(|(n, _)| n == "manifest.json").unwrap().1).unwrap();
    for digest in manifest["entries"].as_array_mut().unwrap() {
        let bytes = &entries.iter().find(|(n, _)| n == digest["path"].as_str().unwrap()).unwrap().1;
        digest["size"] = (bytes.len() as u64).into();
        digest["sha256"] = sha256_hex(bytes).into();
    }
    entries.iter_mut().find(|(n, _)| n == "manifest.json").unwrap().1 = serde_json::to_vec(&manifest).unwrap();
    write_zip(&temp.project(), entries);
    assert!(
        load_project(&temp.project(), &temp.workspace())
            .unwrap_err()
            .message
            .contains("without confirmed permission")
    );
}

#[test]
fn raw_duplicate_entries_and_unknown_unix_type_are_refused() {
    let temp = Temp::new();
    let first = format!("assets/{}.png", "a".repeat(64));
    let second = format!("assets/{}.png", "b".repeat(64));
    write_zip(&temp.project(), vec![(first.clone(), vec![1]), (second.clone(), vec![2])]);
    let mut bytes = fs::read(temp.project()).unwrap();
    for index in 0..=bytes.len() - second.len() {
        if bytes[index..index + second.len()] == *second.as_bytes() {
            bytes[index..index + first.len()].copy_from_slice(first.as_bytes());
        }
    }
    fs::write(temp.project(), bytes).unwrap();
    assert!(
        load_project(&temp.project(), &temp.workspace())
            .unwrap_err()
            .message
            .contains("duplicate")
    );
    saved_after_remove(&temp);
    let mut bytes = fs::read(temp.project()).unwrap();
    let central = bytes.windows(4).position(|s| s == [0x50, 0x4b, 0x01, 0x02]).unwrap();
    bytes[central + 38..central + 42].copy_from_slice(&(0o120777_u32 << 16).to_le_bytes());
    fs::write(temp.project(), bytes).unwrap();
    assert!(
        load_project(&temp.project(), &temp.workspace())
            .unwrap_err()
            .message
            .contains("links")
    );
}
fn saved_after_remove(temp: &Temp) {
    fs::remove_file(temp.project()).unwrap();
    saved(temp);
}

#[test]
fn compression_encryption_huge_sizes_truncation_and_crc_damage_are_refused() {
    for change in 0..6 {
        let temp = Temp::new();
        saved(&temp);
        let mut bytes = fs::read(temp.project()).unwrap();
        let central = bytes.windows(4).position(|s| s == [0x50, 0x4b, 0x01, 0x02]).unwrap();
        match change {
            0 => {
                bytes[8..10].copy_from_slice(&8_u16.to_le_bytes());
                bytes[central + 10..central + 12].copy_from_slice(&8_u16.to_le_bytes());
            }
            1 => {
                bytes[6..8].copy_from_slice(&1_u16.to_le_bytes());
                bytes[central + 8..central + 10].copy_from_slice(&1_u16.to_le_bytes());
            }
            2 => {
                let huge = (project_store::MAX_JSON_BYTES + 1) as u32;
                bytes[central + 20..central + 24].copy_from_slice(&huge.to_le_bytes());
                bytes[central + 24..central + 28].copy_from_slice(&huge.to_le_bytes());
            }
            3 => bytes.truncate(bytes.len() - 5),
            4 => bytes[50] ^= 1,
            5 => {
                let end = bytes.len() - 22;
                let original = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap());
                bytes.splice(central..central, b"unknown".iter().copied());
                let end = bytes.len() - 22;
                bytes[end + 16..end + 20].copy_from_slice(&(original + 7).to_le_bytes());
            }
            _ => unreachable!(),
        }
        fs::write(temp.project(), bytes).unwrap();
        let workspace = temp.workspace();
        assert!(load_project(&temp.project(), &workspace).is_err());
        assert_eq!(fs::read_dir(workspace).unwrap().count(), 0);
    }
}

#[test]
fn recovery_uses_last_complete_record_and_preserves_original() {
    let temp = Temp::new();
    saved(&temp);
    let original = fs::read(temp.project()).unwrap();
    let workspace = temp.workspace();
    let doc = simple();
    write_recovery(&workspace, &doc, &resources()).unwrap();
    let mut changed = doc.clone();
    changed.revision += 1;
    changed.title = "last committed edit".into();
    write_recovery(&workspace, &changed, &resources()).unwrap();
    let journal = workspace.join("recovery/journal.bin");
    fs::OpenOptions::new()
        .append(true)
        .open(&journal)
        .unwrap()
        .write_all(b"SGRJ\x10")
        .unwrap();
    let recovered = recover_project(&workspace).unwrap();
    assert_eq!(recovered.document, changed);
    assert!(
        recovered
            .issues
            .iter()
            .any(|i| i.code == Code::SaveFailed && i.severity == Severity::Warning)
    );
    assert_eq!(fs::read(temp.project()).unwrap(), original);
    assert!(fs::read(journal).unwrap().ends_with(b"SGRJ\x10"));
}

#[test]
fn recovery_ignores_a_corrupted_latest_checkpoint() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    let doc = simple();
    write_recovery(&workspace, &doc, &resources()).unwrap();
    let mut changed = doc.clone();
    changed.revision += 1;
    write_recovery(&workspace, &changed, &resources()).unwrap();
    fs::write(workspace.join("recovery/checkpoint-00000000000000000002.siliqcg"), b"torn").unwrap();
    let recovered = recover_project(&workspace).unwrap();
    assert_eq!(recovered.document, doc);
    assert!(recovered.issues.iter().any(|i| i.code == Code::SaveFailed));
}

#[test]
fn same_machine_recovery_keeps_reference_font_outside_portable_packages() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    let mut doc = fixture();
    doc.fonts[0].package_policy = PackagePolicy::ReferenceOnly;
    write_recovery(&workspace, &doc, &resources()).unwrap();
    let checkpoint = workspace.join("recovery/checkpoint-00000000000000000001.siliqcg");
    assert!(
        !archive_entries(&checkpoint)
            .iter()
            .any(|(name, _)| name.starts_with("fonts/") && name != "fonts/manifest.json")
    );
    let loaded = recover_project(&workspace).unwrap();
    assert_eq!(loaded.document, doc);
    assert!(loaded.issues.is_empty());
    let font = &doc.fonts[0];
    let (bytes, _) = loaded
        .resources
        .read("fonts", font.sha256.as_ref().unwrap(), &["otf"], 64 * 1024 * 1024)
        .unwrap();
    assert_eq!(sha256_hex(&bytes), *font.sha256.as_ref().unwrap());
    let portable = temp.project();
    save_project(&portable, &loaded.document, &loaded.resources, false).unwrap();
    assert!(
        !archive_entries(&portable)
            .iter()
            .any(|(name, _)| name.starts_with("fonts/") && name != "fonts/manifest.json")
    );
}

#[test]
fn recovery_retains_a_bounded_number_of_durable_checkpoints() {
    let temp = Temp::new();
    let workspace = temp.workspace();
    let mut doc = simple();
    for _ in 0..12 {
        write_recovery(&workspace, &doc, &resources()).unwrap();
        doc.revision += 1;
    }
    let files = fs::read_dir(workspace.join("recovery"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("checkpoint-"))
        .count();
    assert_eq!(files, 8);
    assert_eq!(recover_project(&workspace).unwrap().document.revision, doc.revision - 1);
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_inputs_outputs_and_workspace_are_refused() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    saved(&temp);
    let alias = temp.0.join("alias.siliqcg");
    symlink(temp.project(), &alias).unwrap();
    assert_eq!(load_project(&alias, &temp.workspace()).unwrap_err().code, Code::PathDenied);
    assert_eq!(
        save_project(&alias, &simple(), &resources(), true).unwrap_err().code,
        Code::PathDenied
    );
    fs::remove_file(&alias).unwrap();
    fs::hard_link(temp.project(), &alias).unwrap();
    assert_eq!(load_project(&alias, &temp.workspace()).unwrap_err().code, Code::PathDenied);
    assert_eq!(
        save_project(&temp.project(), &simple(), &resources(), true).unwrap_err().code,
        Code::PathDenied
    );
    fs::remove_file(alias).unwrap();
    let outside = Temp::new();
    let link = temp.0.join("linked-workspace");
    symlink(&outside.0, &link).unwrap();
    assert_eq!(load_project(&temp.project(), &link).unwrap_err().code, Code::PathDenied);
    assert_eq!(fs::read_dir(&outside.0).unwrap().count(), 0);
}

#[test]
fn directory_handle_api_refuses_escape_names_and_survives_rename() {
    let temp = Temp::new();
    let source = temp.0.join("selected");
    fs::create_dir(&source).unwrap();
    let dir = File::open(&source).unwrap();
    let moved = temp.0.join("renamed");
    fs::rename(&source, &moved).unwrap();
    for name in ["../escape.siliqcg", "/escape.siliqcg", "child/name.siliqcg"] {
        assert_eq!(
            project_store::save_project_in(&dir, std::ffi::OsStr::new(name), &simple(), &resources(), false)
                .unwrap_err()
                .code,
            Code::PathDenied
        );
    }
    project_store::save_project_in(&dir, std::ffi::OsStr::new("safe.siliqcg"), &simple(), &resources(), false).unwrap();
    assert!(moved.join("safe.siliqcg").exists());
    assert!(!source.exists());
}

#[cfg(unix)]
#[test]
fn ancestor_symlinks_and_parent_traversal_are_refused_for_exact_directory_lookup() {
    use std::os::unix::fs::symlink;
    let temp = Temp::new();
    let actual = temp.0.join("actual");
    let nested = actual.join("nested");
    fs::create_dir_all(&nested).unwrap();
    let alias = temp.0.join("alias");
    symlink(&actual, &alias).unwrap();
    let destination = alias.join("nested/blocked.siliqcg");
    assert_eq!(
        save_project(&destination, &simple(), &resources(), false).unwrap_err().code,
        Code::PathDenied
    );
    assert!(!nested.join("blocked.siliqcg").exists());
    let destination = nested.join("../blocked.siliqcg");
    assert_eq!(
        save_project(&destination, &simple(), &resources(), false).unwrap_err().code,
        Code::PathDenied
    );
    assert!(!actual.join("blocked.siliqcg").exists());
    save_project(&nested.join("allowed.siliqcg"), &simple(), &resources(), false).unwrap();
    let recovery = nested.join("recovery-root");
    fs::create_dir(&recovery).unwrap();
    write_recovery(&recovery, &simple(), &resources()).unwrap();
    assert_eq!(recover_project(&recovery).unwrap().document, simple());
}
