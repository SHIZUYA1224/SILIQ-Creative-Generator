//! Only app-owned cache is inspected/deleted. Native grants, live sessions,
//! unfinished immutable jobs, and the latest recovery remain protected.
use crate::{
    batch::{BatchHost, SharedBatches},
    blocking, grants,
    host::{Host, HostResult, SharedHost, error},
    lock,
};
use design_model::Code;
use rustix::fs::{self, AtFlags, Dir, FileType, Mode, OFlags};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    ffi::{OsStr, OsString},
    fs::File,
    os::unix::ffi::OsStrExt,
    path::PathBuf,
};
use tauri::State;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatus {
    pub display_path: String,
    pub workspace_bytes: u64,
    pub recovery_bytes: u64,
    pub job_bytes: u64,
    pub workspace_count: usize,
    pub saved_job_count: usize,
    pub protected_count: usize,
    pub cleanable_workspace_bytes: u64,
    pub cleanable_recovery_bytes: u64,
    pub cleanable_job_bytes: u64,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CleanupKind {
    Recovery,
    Jobs,
    Workspaces,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReceipt {
    removed_count: usize,
    removed_bytes: u64,
    protected_count: usize,
    status: StorageStatus,
}

fn io_error() -> Vec<design_model::Issue> {
    error(
        Code::PathDenied,
        "app-owned storage could not be inspected safely",
        "Check the local app workspace permissions.",
    )
}
fn names(dir: &File) -> HostResult<Vec<OsString>> {
    Dir::read_from(dir)
        .map_err(|_| io_error())?
        .filter_map(|entry| match entry {
            Ok(entry) if !matches!(entry.file_name().to_bytes(), b"." | b"..") => {
                Some(Ok(OsStr::from_bytes(entry.file_name().to_bytes()).to_owned()))
            }
            Ok(_) => None,
            Err(_) => Some(Err(io_error())),
        })
        .collect()
}
fn child(dir: &File, name: &OsStr) -> HostResult<File> {
    let file: File = fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(|_| io_error())?
    .into();
    grants::verify_local(&file).map_err(|i| vec![i])?;
    Ok(file)
}
fn exists_dir(dir: &File, name: &OsStr) -> bool {
    fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW).is_ok_and(|s| FileType::from_raw_mode(s.st_mode) == FileType::Directory)
}
fn tree_bytes(dir: &File, depth: usize, entries: &mut usize) -> HostResult<u64> {
    if depth > 64 {
        return Err(error(
            Code::ResourceLimit,
            "app storage nesting exceeds the inspection limit",
            "Preserve the workspace and inspect it manually.",
        ));
    }
    let mut total = 0u64;
    for name in names(dir)? {
        *entries += 1;
        if *entries > 100_000 {
            return Err(error(
                Code::ResourceLimit,
                "app storage exceeds the inspection entry limit",
                "Preserve the workspace and inspect it manually.",
            ));
        }
        let stat = fs::statat(dir, &name, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| io_error())?;
        match FileType::from_raw_mode(stat.st_mode) {
            FileType::Directory => {
                total = total.saturating_add(tree_bytes(&child(dir, &name)?, depth + 1, entries)?);
            }
            FileType::RegularFile => {
                total = total.saturating_add(stat.st_size.max(0) as u64);
            }
            _ => {} // Never follow symlinks or read devices/FIFOs.
        }
    }
    Ok(total)
}
fn size(dir: &File) -> HostResult<u64> {
    tree_bytes(dir, 0, &mut 0)
}
fn managed_workspace(name: &str) -> bool {
    if let Some(hex) = name.strip_prefix("document-") {
        return hex.len() == 24 && hex.bytes().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase());
    }
    name.strip_prefix("project-").is_some_and(|suffix| {
        suffix.split('-').count() == 3 && suffix.split('-').all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
    })
}
fn latest(host: &Host) -> HostResult<Option<PathBuf>> {
    let path = host.recovery.join("latest");
    if std::fs::symlink_metadata(&path).is_err() {
        return Ok(None);
    }
    let bytes = grants::read_selected(&path, 512).map_err(|i| vec![i])?;
    let name = std::str::from_utf8(&bytes).map_err(|_| io_error())?;
    if name.is_empty()
        || !name
            .split('/')
            .all(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-'))
    {
        return Err(io_error());
    }
    Ok(Some(host.workspace.join(name)))
}
fn protections(host: &Host, batches: &BatchHost) -> HostResult<BTreeSet<PathBuf>> {
    let mut paths = batches.protected_paths();
    paths.extend(host.sessions.values().map(|r| r.workspace.clone()));
    paths.extend(host.in_flight.values().cloned());
    if let Some(path) = latest(host)? {
        paths.insert(path);
    }
    Ok(paths)
}
fn inspect(host: &Host, batches: &BatchHost) -> HostResult<StorageStatus> {
    let root = grants::open_local(&host.workspace, true).map_err(|i| vec![i])?;
    let protected = protections(host, batches)?;
    let mut status = StorageStatus {
        display_path: host.workspace.display().to_string(),
        workspace_bytes: size(&root)?,
        recovery_bytes: 0,
        job_bytes: 0,
        workspace_count: 0,
        saved_job_count: 0,
        protected_count: protected.len(),
        cleanable_workspace_bytes: 0,
        cleanable_recovery_bytes: 0,
        cleanable_job_bytes: 0,
    };
    for name in names(&root)? {
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !managed_workspace(name_str) || !exists_dir(&root, &name) {
            continue;
        }
        status.workspace_count += 1;
        let dir = child(&root, &name)?;
        let has_recovery = exists_dir(&dir, OsStr::new("recovery"));
        if has_recovery {
            let bytes = size(&child(&dir, OsStr::new("recovery"))?)?;
            status.recovery_bytes += bytes;
            if !protected.contains(&host.workspace.join(&name)) {
                status.cleanable_recovery_bytes += bytes;
            }
        } else if !protected.contains(&host.workspace.join(&name)) {
            status.cleanable_workspace_bytes += size(&dir)?;
        }
    }
    if exists_dir(&root, OsStr::new("batch-snapshots")) {
        let jobs = child(&root, OsStr::new("batch-snapshots"))?;
        status.job_bytes = size(&jobs)?;
        for name in names(&jobs)? {
            let Some(id) = name.to_str() else {
                continue;
            };
            if !exists_dir(&jobs, &name) {
                continue;
            }
            status.saved_job_count += 1;
            let path = host.workspace.join("batch-snapshots").join(id);
            if !protected.contains(&path) && crate::batch::completed_snapshot(&host.workspace.join("batch-snapshots"), id) {
                status.cleanable_job_bytes += size(&child(&jobs, &name)?)?;
            }
        }
    }
    Ok(status)
}

fn remove_tree(parent: &File, name: &OsStr, depth: usize) -> HostResult<()> {
    if depth > 64 {
        return Err(io_error());
    }
    let dir = child(parent, name)?;
    for entry in names(&dir)? {
        let stat = fs::statat(&dir, &entry, AtFlags::SYMLINK_NOFOLLOW).map_err(|_| io_error())?;
        if FileType::from_raw_mode(stat.st_mode) == FileType::Directory {
            remove_tree(&dir, &entry, depth + 1)?;
        } else {
            if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile && stat.st_nlink != 1 {
                return Err(io_error());
            }
            fs::unlinkat(&dir, &entry, AtFlags::empty()).map_err(|_| io_error())?;
        }
    }
    fs::unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(|_| io_error())?;
    fs::fsync(parent).map_err(|_| io_error())?;
    Ok(())
}
fn clean(host: &Host, batches: &BatchHost, kind: CleanupKind) -> HostResult<CleanupReceipt> {
    let root = grants::open_local(&host.workspace, true).map_err(|i| vec![i])?;
    let protected = protections(host, batches)?;
    let mut removed_count = 0;
    let mut removed_bytes = 0;
    match kind {
        CleanupKind::Recovery | CleanupKind::Workspaces => {
            for name in names(&root)? {
                let Some(id) = name.to_str() else {
                    continue;
                };
                if !managed_workspace(id) || protected.contains(&host.workspace.join(id)) || !exists_dir(&root, &name) {
                    continue;
                }
                let dir = child(&root, &name)?;
                let has_recovery = exists_dir(&dir, OsStr::new("recovery"));
                if matches!(kind, CleanupKind::Recovery) && has_recovery {
                    let recovery = child(&dir, OsStr::new("recovery"))?;
                    removed_bytes += size(&recovery)?;
                    remove_tree(&dir, OsStr::new("recovery"), 0)?;
                    removed_count += 1;
                } else if matches!(kind, CleanupKind::Workspaces) && !has_recovery {
                    removed_bytes += size(&dir)?;
                    remove_tree(&root, &name, 0)?;
                    removed_count += 1;
                }
            }
        }
        CleanupKind::Jobs => {
            if exists_dir(&root, OsStr::new("batch-snapshots")) {
                let jobs = child(&root, OsStr::new("batch-snapshots"))?;
                for name in names(&jobs)? {
                    let Some(id) = name.to_str() else {
                        continue;
                    };
                    let path = host.workspace.join("batch-snapshots").join(id);
                    if protected.contains(&path) || !crate::batch::completed_snapshot(&host.workspace.join("batch-snapshots"), id) {
                        continue;
                    }
                    let dir = child(&jobs, &name)?;
                    removed_bytes += size(&dir)?;
                    remove_tree(&jobs, &name, 0)?;
                    removed_count += 1;
                }
            }
        }
    }
    Ok(CleanupReceipt {
        removed_count,
        removed_bytes,
        protected_count: protected.len(),
        status: inspect(host, batches)?,
    })
}

#[tauri::command]
pub async fn storage_status(state: State<'_, SharedHost>, batches: State<'_, SharedBatches>) -> HostResult<StorageStatus> {
    let shared = state.inner().clone();
    let shared_batches = batches.inner().clone();
    blocking(move || storage_status_plain(&shared, &shared_batches)).await
}

pub(crate) fn storage_status_plain(shared: &SharedHost, shared_batches: &SharedBatches) -> HostResult<StorageStatus> {
    let host = lock(shared)?;
    let batches = shared_batches.lock().map_err(|_| io_error())?;
    inspect(&host, &batches)
}

#[tauri::command]
pub async fn cleanup_storage(
    state: State<'_, SharedHost>,
    batches: State<'_, SharedBatches>,
    kind: CleanupKind,
) -> HostResult<CleanupReceipt> {
    let shared = state.inner().clone();
    let shared_batches = batches.inner().clone();
    blocking(move || cleanup_storage_plain(&shared, &shared_batches, kind)).await
}

pub(crate) fn cleanup_storage_plain(shared: &SharedHost, shared_batches: &SharedBatches, kind: CleanupKind) -> HostResult<CleanupReceipt> {
    let host = lock(shared)?;
    let batches = shared_batches.lock().map_err(|_| io_error())?;
    clean(&host, &batches, kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cleanup_preserves_live_and_latest_recovery_and_unselected_symlink_targets() {
        let workspace = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(crate::host::opaque_id("storage-test"));
        let mut host = Host::new(workspace.clone(), PathBuf::new()).unwrap();
        let live = host.new_document().unwrap();
        let old = host.new_document().unwrap();
        let last = host.new_document().unwrap();
        let old_path = host.record(&old.session_id).unwrap().workspace.clone();
        let last_path = host.record(&last.session_id).unwrap().workspace.clone();
        std::fs::create_dir(old_path.join("recovery")).unwrap();
        std::fs::write(old_path.join("recovery/checkpoint"), b"old").unwrap();
        std::fs::create_dir(last_path.join("recovery")).unwrap();
        std::fs::write(last_path.join("recovery/checkpoint"), b"latest").unwrap();
        grants::DirectoryGrant::new(host.recovery.clone())
            .unwrap()
            .replace_control("latest", last_path.file_name().unwrap().to_str().unwrap().as_bytes())
            .unwrap();
        host.sessions.remove(&old.session_id);
        host.sessions.remove(&last.session_id);
        let outside = workspace.join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("keep"), b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, old_path.join("fonts/escape")).unwrap();
        let batches = BatchHost::default();
        host.in_flight.insert("active-export".into(), old_path.clone());
        assert_eq!(clean(&host, &batches, CleanupKind::Recovery).unwrap().removed_count, 0);
        assert!(old_path.join("recovery/checkpoint").exists());
        host.in_flight.remove("active-export");
        assert_eq!(clean(&host, &batches, CleanupKind::Recovery).unwrap().removed_count, 1);
        assert!(!old_path.join("recovery").exists());
        assert!(last_path.join("recovery/checkpoint").exists());
        assert_eq!(clean(&host, &batches, CleanupKind::Workspaces).unwrap().removed_count, 1);
        assert!(!old_path.exists());
        assert!(last_path.exists());
        assert!(host.record(&live.session_id).unwrap().workspace.exists());
        assert_eq!(std::fs::read(outside.join("keep")).unwrap(), b"keep");
        std::fs::remove_dir_all(workspace).unwrap();
    }
}
