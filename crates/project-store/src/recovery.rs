//! A package checkpoint precedes each journal commit. A torn journal suffix
//! never makes an incomplete checkpoint the authoritative document.
use super::*;
use rustix::fs::{FlockOperation, Mode, OFlags};

const HEADER: usize = 72; // magic (4), JSON length (4), SHA-256 hex (64)
const MAX_RECORD: usize = 2048;
const MAX_JOURNAL: u64 = 20 * 1024 * 1024;
/// Retain enough complete generations for rollback without copying a project
/// indefinitely on every edit. The journal remains an auditable commit ledger.
pub const RETAINED_CHECKPOINTS: usize = 8;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    sequence: u64,
    revision: u64,
    document_id: String,
    package_name: String,
    package_sha256: String,
}

/// Called after committed transactions, with the app-owned session workspace.
/// Original user packages are never touched by checkpoints or recovery.
pub fn write_recovery(workspace: &Path, document: &DesignDocument, resources: &ResourceStore) -> Result<SaveReceipt, Issue> {
    let dir = recovery_dir(workspace, true)?;
    let mut journal = journal(&dir, true)?;
    rustix::fs::flock(&journal, FlockOperation::NonBlockingLockExclusive).map_err(|_| fs::io_error("recovery is already being written"))?;
    let (records, damaged) = records(&mut journal)?;
    if damaged {
        return Err(invalid(
            "recover the last complete journal record before creating another checkpoint",
        ));
    }
    if records
        .last()
        .is_some_and(|r| r.document_id != document.document_id || r.revision >= document.revision)
    {
        return Err(Issue::error(
            Code::RevisionConflict,
            "recovery journal belongs to another document or a newer revision",
            "Use this document's session workspace and its current committed revision.",
        ));
    }
    let sequence = records.last().map_or(1, |r| r.sequence + 1);
    // Reference-only font files may remain in the private same-machine
    // workspace after a crash. They are never written into a portable ZIP.
    preserve_local_fonts(&dir, document, resources)?;
    let package_name = format!("checkpoint-{sequence:020}.siliqcg");
    let receipt = save_in(&dir, std::ffi::OsStr::new(&package_name), document, resources, false)?;
    let record = Record {
        sequence,
        revision: document.revision,
        document_id: document.document_id.clone(),
        package_name,
        package_sha256: receipt.checksum.clone(),
    };
    let payload = serde_json::to_vec(&record).map_err(fs::io_error)?;
    if payload.len() > MAX_RECORD || journal.metadata().map_err(fs::io_error)?.len() + HEADER as u64 + payload.len() as u64 > MAX_JOURNAL {
        return Err(limit());
    }
    let mut frame = Vec::with_capacity(HEADER + payload.len());
    frame.extend_from_slice(b"SGRJ");
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(sha256_hex(&payload).as_bytes());
    frame.extend_from_slice(&payload);
    journal.seek(SeekFrom::End(0)).map_err(fs::io_error)?;
    journal.write_all(&frame).map_err(fs::io_error)?;
    journal.sync_all().map_err(fs::io_error)?;
    rustix::fs::fsync(&dir).map_err(fs::io_error)?;
    // Only after the new journal commit is durable can app-owned older
    // checkpoints be reclaimed. Never remove either of the latest two.
    for old in records
        .iter()
        .take(records.len().saturating_add(1).saturating_sub(RETAINED_CHECKPOINTS))
    {
        fs::remove(&dir, &old.package_name);
    }
    rustix::fs::fsync(&dir).map_err(fs::io_error)?;
    Ok(receipt)
}

/// Restores the latest fully verified checkpoint. A torn/corrupt suffix is
/// reported, and the journal and original project remain available unchanged.
pub fn recover_project(workspace: &Path) -> Result<LoadedProject, Issue> {
    let dir = recovery_dir(workspace, false)?;
    let mut journal = journal(&dir, false)?;
    rustix::fs::flock(&journal, FlockOperation::NonBlockingLockShared).map_err(|_| fs::io_error("recovery is being written"))?;
    let (records, mut damaged) = records(&mut journal)?;
    for record in records.iter().rev() {
        let attempt = (|| {
            let mut file = fs::read(&dir, std::ffi::OsStr::new(&record.package_name))?;
            if file_digest(&mut file)? != record.package_sha256 {
                return Err(invalid("checkpoint package checksum does not match its journal"));
            }
            let checked = package::read(file)?;
            if checked.document.revision != record.revision || checked.document.document_id != record.document_id {
                return Err(invalid("checkpoint document differs from its journal"));
            }
            let mut loaded = extract(checked, workspace)?;
            restore_local_fonts(workspace, &mut loaded)?;
            Ok(loaded)
        })();
        match attempt {
            Ok(mut loaded) => {
                if damaged {
                    loaded.issues.push(Issue::warning(Code::SaveFailed, "an incomplete or damaged recovery suffix was ignored; the last complete checkpoint was recovered", "Review the recovered revision and save it to a new project file. The original project and recovery journal were preserved."));
                }
                return Ok(loaded);
            }
            Err(_) => damaged = true,
        }
    }
    Err(invalid("no complete, verified recovery checkpoint is available"))
}

fn local_font_dir(dir: &File) -> Result<File, Issue> {
    match rustix::fs::mkdirat(dir, "fonts", Mode::RUSR | Mode::WUSR | Mode::XUSR) {
        Ok(()) => {}
        Err(e) if e == rustix::io::Errno::EXIST => {}
        Err(e) => return Err(fs::io_error(e)),
    }
    rustix::fs::openat(
        dir,
        "fonts",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(Into::into)
    .map_err(|_| fs::denied())
}

fn preserve_local_fonts(dir: &File, document: &DesignDocument, resources: &ResourceStore) -> Result<(), Issue> {
    let fonts = local_font_dir(dir)?;
    let mut total = 0_u64;
    for font in &document.fonts {
        if font.package_policy != PackagePolicy::ReferenceOnly || font.state != ResolveState::Resolved {
            continue;
        }
        let sha = font
            .sha256
            .as_deref()
            .ok_or_else(|| invalid("local recovery font hash is missing"))?;
        let (bytes, ext) = resources
            .read("fonts", sha, &["ttf", "otf", "ttc"], MAX_FONT_BYTES)
            .map_err(|e| e.into_issue("local recovery font", Code::FontMissing).font(&font.id))?;
        total = total.checked_add(bytes.len() as u64).ok_or_else(limit)?;
        if total > MAX_PACKAGE_BYTES {
            return Err(limit());
        }
        let name = format!("{sha}.{ext}");
        match rustix::fs::statat(&fonts, name.as_str(), rustix::fs::AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => {
                let mut existing = fs::read(&fonts, std::ffi::OsStr::new(&name))?;
                let mut contents = Vec::new();
                Read::by_ref(&mut existing)
                    .take(MAX_FONT_BYTES + 1)
                    .read_to_end(&mut contents)
                    .map_err(fs::io_error)?;
                if sha256_hex(&contents) != sha {
                    return Err(invalid("local recovery font has changed"));
                }
            }
            Err(e) if e == rustix::io::Errno::NOENT => {
                // Commit a complete file; a crash may leave an unreferenced
                // temporary file, never a partial content-addressed font.
                let temporary = unique(".font");
                let result = (|| {
                    let mut file = fs::create(&fonts, &temporary)?;
                    file.write_all(&bytes).map_err(fs::io_error)?;
                    file.sync_all().map_err(fs::io_error)?;
                    fs::commit(&fonts, &temporary, std::ffi::OsStr::new(&name), false)
                })();
                fs::remove(&fonts, &temporary);
                result?;
            }
            Err(e) => return Err(fs::io_error(e)),
        }
    }
    Ok(())
}

fn restore_local_fonts(workspace: &Path, loaded: &mut LoadedProject) -> Result<(), Issue> {
    let resources = match ResourceStore::open(&workspace.join("recovery")) {
        Ok(store) => store,
        Err(_) => return Ok(()),
    };
    let root = fs::directory(&loaded.workspace)?;
    let fonts = local_font_dir(&root)?;
    for font in &loaded.document.fonts {
        if font.package_policy != PackagePolicy::ReferenceOnly || font.state != ResolveState::Resolved {
            continue;
        }
        let Some(sha) = font.sha256.as_deref() else {
            continue;
        };
        let Ok((bytes, ext)) = resources.read("fonts", sha, &["ttf", "otf", "ttc"], MAX_FONT_BYTES) else {
            continue;
        };
        let name = format!("{sha}.{ext}");
        // Another explicitly embeddable face may use the same font bytes.
        if rustix::fs::statat(&fonts, name.as_str(), rustix::fs::AtFlags::SYMLINK_NOFOLLOW).is_err() {
            let mut file = fs::create(&fonts, &name)?;
            file.write_all(&bytes).map_err(fs::io_error)?;
            file.sync_all().map_err(fs::io_error)?;
        }
        loaded
            .issues
            .retain(|issue| !(issue.code == Code::FontMissing && issue.location.font_id.as_deref() == Some(font.id.as_str())));
    }
    rustix::fs::fsync(&fonts).map_err(fs::io_error)?;
    Ok(())
}

fn recovery_dir(workspace: &Path, create: bool) -> Result<File, Issue> {
    let root = fs::directory(workspace)?;
    if create {
        match rustix::fs::mkdirat(&root, "recovery", Mode::RUSR | Mode::WUSR | Mode::XUSR) {
            Ok(()) => {}
            Err(e) if e == rustix::io::Errno::EXIST => {}
            Err(e) => return Err(fs::io_error(e)),
        }
    }
    rustix::fs::openat(
        &root,
        "recovery",
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(Into::into)
    .map_err(|_| fs::denied())
}

fn journal(dir: &File, create: bool) -> Result<File, Issue> {
    let flags =
        OFlags::RDWR | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK | if create { OFlags::CREATE } else { OFlags::empty() };
    let file: File = rustix::fs::openat(dir, "journal.bin", flags, Mode::RUSR | Mode::WUSR)
        .map_err(fs::io_error)?
        .into();
    fs::regular(&file)?;
    Ok(file)
}

fn records(file: &mut File) -> Result<(Vec<Record>, bool), Issue> {
    let size = file.metadata().map_err(fs::io_error)?.len();
    if size > MAX_JOURNAL {
        return Err(limit());
    }
    file.seek(SeekFrom::Start(0)).map_err(fs::io_error)?;
    let mut bytes = Vec::with_capacity(size as usize);
    file.take(MAX_JOURNAL + 1).read_to_end(&mut bytes).map_err(fs::io_error)?;
    let mut records: Vec<Record> = Vec::new();
    let mut position = 0;
    while position < bytes.len() {
        if bytes.len() - position < HEADER || &bytes[position..position + 4] != b"SGRJ" {
            return Ok((records, true));
        }
        let size = u32::from_le_bytes(bytes[position + 4..position + 8].try_into().unwrap()) as usize;
        if size > MAX_RECORD || size == 0 || position + HEADER + size > bytes.len() {
            return Ok((records, true));
        }
        let payload = &bytes[position + HEADER..position + HEADER + size];
        if sha256_hex(payload).as_bytes() != &bytes[position + 8..position + HEADER] {
            return Ok((records, true));
        }
        let record: Record = match serde_json::from_slice(payload) {
            Ok(record) => record,
            Err(_) => return Ok((records, true)),
        };
        if record.sequence != records.last().map_or(1, |r| r.sequence + 1)
            || record.package_name != format!("checkpoint-{:020}.siliqcg", record.sequence)
            || !design_model::validate::is_sha256_hex(&record.package_sha256)
            || records
                .last()
                .is_some_and(|r| r.document_id != record.document_id || r.revision >= record.revision)
        {
            return Ok((records, true));
        }
        records.push(record);
        position += HEADER + size;
    }
    Ok((records, false))
}
