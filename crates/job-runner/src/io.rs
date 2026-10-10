//! Writes and publication are relative to a retained output directory handle.
use design_model::{Code, Issue};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path};

pub(crate) fn error(error: impl std::fmt::Display) -> Issue {
    Issue::error(
        Code::SaveFailed,
        format!("batch file operation failed: {error}"),
        "Check the selected output directory and available disk space.",
    )
}
pub(crate) fn denied() -> Issue {
    Issue::error(
        Code::PathDenied,
        "batch output must stay in the selected local directory",
        "Choose a real local directory without symbolic links.",
    )
}
pub(crate) fn collision(name: &str) -> Issue {
    Issue::error(
        Code::OutputCollision,
        format!("{name} already exists in the output directory"),
        "Choose a new output directory; existing files are never overwritten.",
    )
}
pub(crate) fn directory(path: &Path) -> Result<File, Issue> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(error)?.join(path)
    };
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = fs::open("/", flags, Mode::empty()).map_err(error)?.into();
    for component in absolute.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => dir = fs::openat(&dir, name, flags, Mode::empty()).map_err(|_| denied())?.into(),
            _ => return Err(denied()),
        }
    }
    Ok(dir)
}
pub(crate) fn validate_directory(file: &File) -> Result<(), Issue> {
    if FileType::from_raw_mode(fs::fstat(file).map_err(error)?.st_mode) != FileType::Directory {
        return Err(denied());
    }
    Ok(())
}
pub(crate) fn exists(dir: &File, name: &str) -> Result<bool, Issue> {
    match fs::statat(dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => Ok(true),
        Err(e) if e == rustix::io::Errno::NOENT => Ok(false),
        Err(e) => Err(error(e)),
    }
}
pub(crate) fn child(dir: &File, name: &str, create: bool) -> Result<File, Issue> {
    if create {
        fs::mkdirat(dir, name, Mode::RUSR | Mode::WUSR | Mode::XUSR)
            .map_err(|e| if e == rustix::io::Errno::EXIST { collision(name) } else { error(e) })?;
    }
    fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(Into::into)
    .map_err(|_| denied())
}
pub(crate) fn read(dir: &File, name: &str, max: u64) -> Result<Vec<u8>, Issue> {
    read_with_peer(dir, name, max, None)
}
pub(crate) fn read_with_peer(dir: &File, name: &str, max: u64, peer: Option<&File>) -> Result<Vec<u8>, Issue> {
    let file: File = fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(error)?
    .into();
    let stat = fs::fstat(&file).map_err(error)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_size < 0 || stat.st_size as u64 > max {
        return Err(denied());
    }
    if stat.st_nlink != 1 {
        // A no-clobber publication briefly has exactly the app-owned staging
        // link and the granted output link. Refuse every other hard link.
        let peer = peer.ok_or_else(denied)?;
        if stat.st_nlink != 2 {
            return Err(denied());
        }
        let other = fs::openat(
            peer,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| denied())?;
        let other_stat = fs::fstat(&other).map_err(error)?;
        if other_stat.st_dev != stat.st_dev || other_stat.st_ino != stat.st_ino || other_stat.st_nlink != 2 {
            return Err(denied());
        }
    }
    let mut bytes = Vec::with_capacity(stat.st_size as usize);
    file.take(max + 1).read_to_end(&mut bytes).map_err(error)?;
    if bytes.len() as u64 > max {
        return Err(denied());
    }
    Ok(bytes)
}
pub(crate) fn new_file(dir: &File, name: &str, bytes: &[u8]) -> Result<(), Issue> {
    let mut file: File = fs::openat(
        dir,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(|e| if e == rustix::io::Errno::EXIST { collision(name) } else { error(e) })?
    .into();
    if let Err(e) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        remove(dir, name);
        return Err(error(e));
    }
    Ok(())
}
pub(crate) fn atomic_json(dir: &File, target: &str, bytes: &[u8], overwrite: bool) -> Result<(), Issue> {
    let temporary = format!(
        ".state-{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        super::next_nonce()
    );
    new_file(dir, &temporary, bytes)?;
    let result = (|| {
        if overwrite {
            if exists(dir, target)? {
                let stat = fs::statat(dir, target, AtFlags::SYMLINK_NOFOLLOW).map_err(error)?;
                if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile || stat.st_nlink != 1 {
                    return Err(denied());
                }
            }
            fs::renameat(dir, temporary.as_str(), dir, target).map_err(error)?;
        } else {
            fs::linkat(dir, temporary.as_str(), dir, target, AtFlags::empty()).map_err(|e| {
                if e == rustix::io::Errno::EXIST {
                    collision(target)
                } else {
                    error(e)
                }
            })?;
            remove(dir, &temporary);
        }
        fs::fsync(dir).map_err(error)
    })();
    remove(dir, &temporary);
    result
}
pub(crate) fn publish(stage: &File, dir: &File, name: &str) -> Result<(), Issue> {
    fs::linkat(stage, name, dir, name, AtFlags::empty())
        .map_err(|e| if e == rustix::io::Errno::EXIST { collision(name) } else { error(e) })?;
    fs::fsync(dir).map_err(error)
}
pub(crate) fn remove(dir: &File, name: &str) {
    let _ = fs::unlinkat(dir, name, AtFlags::empty());
}
pub(crate) fn remove_child(dir: &File, name: &str) {
    let _ = fs::unlinkat(dir, name, AtFlags::REMOVEDIR);
}
