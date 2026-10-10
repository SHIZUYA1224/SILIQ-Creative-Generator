//! Portable, versioned project packages and crash recovery. The desktop host
//! supplies OS-selected paths and an app-owned workspace; these APIs are never
//! exposed as arbitrary path commands to the WebView.
//!
//! Version 1 writes a deterministic Stored ZIP. It intentionally rejects other
//! compression methods, encryption, ZIP64, extra fields, unknown files and
//! links. Original assets are preserved; only embed-confirmed fonts travel.

mod fs;
mod package;
mod recovery;

use design_model::{Code, DesignDocument, Font, Issue, PackagePolicy, ResolveState, validate};
use design_render::resources::{MAX_FONT_BYTES, MAX_IMAGE_BYTES, ResourceStore, sha256_hex};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use recovery::{recover_project, write_recovery};

pub const PACKAGE_FORMAT: &str = "siliq-creative-generator-package";
pub const FORMAT_VERSION: &str = "1.0.0";
pub const ENGINE_VERSION: &str = "0.0.1";
pub const MAX_JSON_BYTES: u64 = 20 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_ENTRIES: usize = 10_000;
static NONCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryDigest {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub format_version: String,
    pub schema_version: String,
    pub minimum_engine_version: String,
    pub document_id: String,
    pub revision: u64,
    /// Every entry except manifest.json itself, sorted by portable ASCII path.
    pub entries: Vec<EntryDigest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontReference {
    pub font: Font,
    /// No original machine path is saved, including for reference-only fonts.
    pub embedded_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FontManifest {
    pub fonts: Vec<FontReference>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveReceipt {
    pub revision: u64,
    pub checksum: String,
}

#[derive(Debug)]
pub struct LoadedProject {
    pub document: DesignDocument,
    pub resources: ResourceStore,
    /// Private host-owned path, never serialized into the IPC read model.
    pub workspace: PathBuf,
    pub manifest: Manifest,
    /// Missing reference-only fonts are blocking render issues, but must not
    /// prevent opening and editing a portable document.
    pub issues: Vec<Issue>,
}

type Entries = BTreeMap<String, Arc<Vec<u8>>>;

/// Save to a new file by default. Overwriting requires the host's explicit
/// confirmation. The existing original survives every failure before commit.
pub fn save_project(path: &Path, document: &DesignDocument, resources: &ResourceStore, overwrite: bool) -> Result<SaveReceipt, Issue> {
    let (dir, name) = fs::parent(path)?;
    save_in(&dir, name, document, resources, overwrite)
}

/// Directory-handle variant for the desktop grant broker. `name` must be one
/// filename, and `directory` must be the retained locally granted handle.
pub fn save_project_in(
    directory: &File,
    name: &std::ffi::OsStr,
    document: &DesignDocument,
    resources: &ResourceStore,
    overwrite: bool,
) -> Result<SaveReceipt, Issue> {
    fs::verify_directory(directory)?;
    save_in(directory, name, document, resources, overwrite)
}

fn save_in(
    dir: &File,
    name: &std::ffi::OsStr,
    document: &DesignDocument,
    resources: &ResourceStore,
    overwrite: bool,
) -> Result<SaveReceipt, Issue> {
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
        || name.to_string_lossy().contains(['\0', '\\'])
    {
        return Err(fs::denied());
    }
    let (manifest, entries) = collect(document, resources)?;
    let temporary = unique(".siliq-save");
    let result = (|| {
        let file = fs::create(dir, &temporary)?;
        let mut file = package::write(file, &manifest, &entries)?;
        file.sync_all().map_err(fs::io_error)?;
        // Re-open through the held directory descriptor and fully inspect the
        // completed package before replacing the original.
        file.seek(SeekFrom::Start(0)).map_err(fs::io_error)?;
        let checked = package::read(file.try_clone().map_err(fs::io_error)?)?;
        if checked.manifest != manifest {
            return Err(invalid("saved package validation did not reproduce its manifest"));
        }
        let checksum = file_digest(&mut file)?;
        fs::commit(dir, &temporary, name, overwrite)?;
        Ok(SaveReceipt {
            revision: document.revision,
            checksum,
        })
    })();
    fs::remove(dir, &temporary);
    result
}

/// Verify before extraction, then write only fixed, verified entry names into
/// a newly created child of an existing app-owned workspace.
pub fn load_project(path: &Path, workspace: &Path) -> Result<LoadedProject, Issue> {
    let (dir, name) = fs::parent(path)?;
    let file = fs::read(&dir, name)?;
    load_project_file(file, workspace)
}

/// Uses the exact file already opened and approved by the desktop grant
/// broker, avoiding a second pathname lookup after the OS selection.
pub fn load_project_file(file: File, workspace: &Path) -> Result<LoadedProject, Issue> {
    fs::regular(&file)?;
    extract(package::read(file)?, workspace)
}

fn extract(checked: package::CheckedPackage, workspace: &Path) -> Result<LoadedProject, Issue> {
    let parent = fs::directory(workspace)?;
    let child = unique("project");
    let dir = fs::mkdir(&parent, &child)?;
    let assets = fs::mkdir(&dir, "assets")?;
    let fonts = fs::mkdir(&dir, "fonts")?;
    // Entry paths came from the fixed package grammar, never from arbitrary
    // relative names or extract_all. No archive directory entry is accepted.
    for (path, bytes) in &checked.entries {
        let (dest, name) = if let Some(name) = path.strip_prefix("assets/") {
            (&assets, name)
        } else if let Some(name) = path.strip_prefix("fonts/") {
            (&fonts, name)
        } else {
            (&dir, path.as_str())
        };
        let mut file = fs::create(dest, name)?;
        file.write_all(bytes).map_err(fs::io_error)?;
        file.sync_all().map_err(fs::io_error)?;
    }
    rustix::fs::fsync(&dir).map_err(fs::io_error)?;
    let root = workspace.join(&child);
    let resources = ResourceStore::open(&root)?;
    let mut issues = Vec::new();
    for font in &checked.document.fonts {
        if font.state == ResolveState::Unresolved
            || font
                .sha256
                .as_ref()
                .is_none_or(|sha| !checked.entries.keys().any(|name| name.starts_with(&format!("fonts/{sha}."))))
        {
            issues.push(
                Issue::error(
                    Code::FontMissing,
                    "the project references a font that is not embedded",
                    "Import the exact local font file; its hash and face must match. Embedding requires confirmed permission.",
                )
                .font(&font.id),
            );
        }
    }
    for asset in &checked.document.assets {
        if asset.state == ResolveState::Unresolved {
            issues.push(
                Issue::error(
                    Code::AssetMissing,
                    "the project has an unresolved asset",
                    "Import the required local asset before exporting.",
                )
                .asset(&asset.id),
            );
        }
    }
    Ok(LoadedProject {
        document: checked.document,
        resources,
        workspace: root,
        manifest: checked.manifest,
        issues,
    })
}

fn collect(document: &DesignDocument, resources: &ResourceStore) -> Result<(Manifest, Entries), Issue> {
    if let Some(issue) = validate(document).into_iter().find(|i| i.severity == design_model::Severity::Error) {
        return Err(issue);
    }
    let mut entries = Entries::new();
    let mut total = 0_u64;
    insert_entry(
        &mut entries,
        &mut total,
        "design.json".into(),
        Arc::new(serde_json::to_vec_pretty(document).map_err(fs::io_error)?),
    )?;
    for asset in &document.assets {
        if asset.state == ResolveState::Resolved {
            let sha = asset.sha256.as_deref().ok_or_else(|| invalid("resolved asset has no hash"))?;
            let ext = asset.mime_hint.extension();
            let path = format!("assets/{sha}.{ext}");
            let (bytes, _) = resources
                .read("assets", sha, &[ext], package::entry_limit(&path)?)
                .map_err(|e| e.into_issue("asset", Code::AssetMissing).asset(&asset.id))?;
            insert_entry(&mut entries, &mut total, path, bytes)?;
        }
    }
    let mut font_refs = Vec::new();
    for font in &document.fonts {
        let embedded_path = if font.state == ResolveState::Resolved && font.package_policy == PackagePolicy::EmbedConfirmed {
            let sha = font.sha256.as_deref().ok_or_else(|| invalid("resolved font has no hash"))?;
            let (bytes, ext) = resources
                .read("fonts", sha, &["ttf", "otf", "ttc"], MAX_FONT_BYTES)
                .map_err(|e| e.into_issue("font", Code::FontMissing).font(&font.id))?;
            let path = format!("fonts/{sha}.{ext}");
            insert_entry(&mut entries, &mut total, path.clone(), bytes)?;
            Some(path)
        } else {
            None
        };
        font_refs.push(FontReference {
            font: font.clone(),
            embedded_path,
        });
    }
    insert_entry(
        &mut entries,
        &mut total,
        "fonts/manifest.json".into(),
        Arc::new(serde_json::to_vec_pretty(&FontManifest { fonts: font_refs }).map_err(fs::io_error)?),
    )?;
    if total > MAX_PACKAGE_BYTES || entries.len() + 1 > MAX_ENTRIES {
        return Err(limit());
    }
    let manifest = Manifest {
        format: PACKAGE_FORMAT.into(),
        format_version: FORMAT_VERSION.into(),
        schema_version: document.schema_version.clone(),
        minimum_engine_version: ENGINE_VERSION.into(),
        document_id: document.document_id.clone(),
        revision: document.revision,
        entries: entries
            .iter()
            .map(|(path, bytes)| EntryDigest {
                path: path.clone(),
                sha256: sha256_hex(bytes),
                size: bytes.len() as u64,
            })
            .collect(),
    };
    let manifest_size = serde_json::to_vec_pretty(&manifest).map_err(fs::io_error)?.len() as u64;
    if manifest_size > MAX_JSON_BYTES || total.checked_add(manifest_size).is_none_or(|size| size > MAX_PACKAGE_BYTES) {
        return Err(limit());
    }
    Ok((manifest, entries))
}

fn insert_entry(entries: &mut Entries, total: &mut u64, path: String, bytes: Arc<Vec<u8>>) -> Result<(), Issue> {
    if bytes.len() as u64 > package::entry_limit(&path)? {
        return Err(limit());
    }
    if entries.contains_key(&path) {
        return Ok(());
    }
    *total = total.checked_add(bytes.len() as u64).ok_or_else(limit)?;
    if *total > MAX_PACKAGE_BYTES || entries.len() + 2 > MAX_ENTRIES {
        return Err(limit());
    }
    entries.insert(path, bytes);
    Ok(())
}

fn invalid(message: &str) -> Issue {
    Issue::error(
        Code::PackageInvalid,
        message,
        "Open an intact supported .siliqcg project. The original file was not changed.",
    )
}
fn limit() -> Issue {
    Issue::error(
        Code::ResourceLimit,
        "project package exceeds a resource limit",
        "Use fewer or smaller assets; packages are limited to 2 GiB, 10,000 entries and 20 MiB JSON.",
    )
}
fn unique(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NONCE.fetch_add(1, Ordering::Relaxed)
    )
}
fn file_digest(file: &mut File) -> Result<String, Issue> {
    // zip's read/write path caps the archive itself. Hashing in chunks avoids
    // retaining a second copy of a potentially large package.
    use sha2::{Digest, Sha256};
    file.seek(SeekFrom::Start(0)).map_err(fs::io_error)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let n = file.read(&mut buffer).map_err(fs::io_error)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_PACKAGE_BYTES + 16 * 1024 * 1024 {
            return Err(limit());
        }
        digest.update(&buffer[..n]);
    }
    Ok(digest.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}
