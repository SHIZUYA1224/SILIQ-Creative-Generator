//! Directory-handle operations keep writes inside the selected directory even
//! if its pathname is renamed. Every component is opened without following links.

use design_model::{Code, Issue};
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use std::fs::File;
use std::path::{Component, Path};

pub(crate) fn io_error(error: impl std::fmt::Display) -> Issue {
    Issue::error(
        Code::SaveFailed,
        format!("project I/O failed: {error}"),
        "Check disk space and the selected directory's permissions.",
    )
}

pub(crate) fn denied() -> Issue {
    Issue::error(
        Code::PathDenied,
        "project paths must use regular local files and real directories",
        "Choose a file in a local directory without symbolic or hard links.",
    )
}

pub(crate) fn directory(path: &Path) -> Result<File, Issue> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_err(io_error)?.join(path)
    };
    if absolute
        .components()
        .any(|component| !matches!(component, Component::RootDir | Component::CurDir | Component::Normal(_)))
    {
        return Err(denied());
    }
    #[cfg(target_os = "macos")]
    {
        // Opening ungranted ancestors is denied by App Sandbox. Darwin's
        // atomic whole-path flag still rejects links in every component.
        let absolute: std::path::PathBuf = absolute.components().filter(|c| *c != Component::CurDir).collect();
        darwin::check_path(&absolute)?;
        let file: File = fs::open(
            &absolute,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW_ANY | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| denied())?
        .into();
        verify_directory(&file)?;
        Ok(file)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut current: File = fs::open("/", flags, Mode::empty()).map_err(io_error)?.into();
        for component in absolute.components() {
            match component {
                Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => current = fs::openat(&current, name, flags, Mode::empty()).map_err(|_| denied())?.into(),
                _ => return Err(denied()),
            }
        }
        Ok(current)
    }
}

pub(crate) fn verify_directory(file: &File) -> Result<(), Issue> {
    if FileType::from_raw_mode(fs::fstat(file).map_err(io_error)?.st_mode) != FileType::Directory {
        return Err(denied());
    }
    #[cfg(target_os = "macos")]
    darwin::verify_local(file)?;
    Ok(())
}

pub(crate) fn parent(path: &Path) -> Result<(File, &std::ffi::OsStr), Issue> {
    let name = path.file_name().ok_or_else(denied)?;
    if name.to_string_lossy().contains('\0') {
        return Err(denied());
    }
    let dir = directory(path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new(".")))?;
    Ok((dir, name))
}

pub(crate) fn read(dir: &File, name: &std::ffi::OsStr) -> Result<File, Issue> {
    #[cfg(target_os = "macos")]
    darwin::check_entry(dir, name)?;
    let f: File = fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|e| if e == rustix::io::Errno::LOOP { denied() } else { io_error(e) })?
    .into();
    regular(&f)?;
    Ok(f)
}

pub(crate) fn regular(f: &File) -> Result<(), Issue> {
    let st = fs::fstat(f).map_err(io_error)?;
    if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile || st.st_nlink != 1 {
        return Err(denied());
    }
    #[cfg(target_os = "macos")]
    darwin::verify_local(f)?;
    Ok(())
}

pub(crate) fn create(dir: &File, name: &str) -> Result<File, Issue> {
    fs::openat(
        dir,
        name,
        OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map(Into::into)
    .map_err(io_error)
}

pub(crate) fn mkdir(dir: &File, name: &str) -> Result<File, Issue> {
    fs::mkdirat(dir, name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(io_error)?;
    let file: File = fs::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(Into::into)
    .map_err(io_error)?;
    verify_directory(&file)?;
    Ok(file)
}

pub(crate) fn remove(dir: &File, name: &str) {
    let _ = fs::unlinkat(dir, name, AtFlags::empty());
}

pub(crate) fn commit(dir: &File, temporary: &str, target: &std::ffi::OsStr, overwrite: bool) -> Result<(), Issue> {
    // Never write through an existing target. Rename replaces the directory
    // entry itself, so a concurrent symlink cannot redirect the write.
    match fs::statat(dir, target, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(st) => {
            if FileType::from_raw_mode(st.st_mode) != FileType::RegularFile || st.st_nlink != 1 {
                return Err(denied());
            }
            #[cfg(target_os = "macos")]
            if st.st_flags & (0x0000_0080 | 0x4000_0000) != 0 {
                return Err(denied());
            }
            if !overwrite {
                return Err(collision());
            }
        }
        Err(e) if e == rustix::io::Errno::NOENT => {}
        Err(e) => return Err(io_error(e)),
    }
    if overwrite {
        fs::renameat(dir, temporary, dir, target).map_err(io_error)?;
    } else {
        // linkat is an atomic no-clobber commit; rename's check-then-write
        // would overwrite a file created after the existence check.
        fs::linkat(dir, temporary, dir, target, AtFlags::empty())
            .map_err(|e| if e == rustix::io::Errno::EXIST { collision() } else { io_error(e) })?;
        fs::unlinkat(dir, temporary, AtFlags::empty()).map_err(io_error)?;
    }
    fs::fsync(dir).map_err(io_error)
}

fn collision() -> Issue {
    Issue::error(
        Code::OutputCollision,
        "the project file already exists",
        "Choose another name or explicitly confirm overwriting this file.",
    )
}

#[cfg(target_os = "macos")]
mod darwin {
    use super::*;
    use std::{
        ffi::CString,
        os::{fd::AsRawFd, unix::ffi::OsStrExt},
    };
    const PROVIDER_FLAGS: u32 = 0x0000_0080 | 0x4000_0000; // UF_OFFLINE / SF_DATALESS

    fn attributes() -> libc::attrlist {
        libc::attrlist {
            bitmapcount: libc::ATTR_BIT_MAP_COUNT,
            reserved: 0,
            commonattr: libc::ATTR_CMN_FLAGS,
            volattr: 0,
            dirattr: 0,
            fileattr: 0,
            forkattr: 0,
        }
    }
    fn check_flags(flags: [u32; 2]) -> Result<(), Issue> {
        if flags[0] != std::mem::size_of_val(&flags) as u32 || flags[1] & PROVIDER_FLAGS != 0 {
            return Err(denied());
        }
        Ok(())
    }
    pub(super) fn check_path(path: &Path) -> Result<(), Issue> {
        refuse_remote_mount(path)?;
        let path = CString::new(path.as_os_str().as_bytes()).map_err(|_| denied())?;
        let mut attrs = attributes();
        let mut flags = [0u32; 2];
        // Metadata-only lookup avoids opening a provider placeholder's content.
        // Both the metadata and subsequent open reject all ancestor symlinks.
        if unsafe {
            libc::getattrlist(
                path.as_ptr(),
                (&mut attrs as *mut libc::attrlist).cast(),
                flags.as_mut_ptr().cast(),
                std::mem::size_of_val(&flags),
                libc::FSOPT_NOFOLLOW | libc::FSOPT_NOFOLLOW_ANY,
            )
        } != 0
        {
            return Err(denied());
        }
        check_flags(flags)
    }
    pub(super) fn check_entry(dir: &File, name: &std::ffi::OsStr) -> Result<(), Issue> {
        verify_local(dir)?;
        let name = CString::new(name.as_bytes()).map_err(|_| denied())?;
        let mut attrs = attributes();
        let mut flags = [0u32; 2];
        // Read no content before rejecting a dataless package/journal/font.
        if unsafe {
            libc::getattrlistat(
                dir.as_raw_fd(),
                name.as_ptr(),
                (&mut attrs as *mut libc::attrlist).cast(),
                flags.as_mut_ptr().cast(),
                std::mem::size_of_val(&flags),
                u64::from(libc::FSOPT_NOFOLLOW | libc::FSOPT_NOFOLLOW_ANY),
            )
        } != 0
        {
            return Err(denied());
        }
        check_flags(flags)
    }
    pub(super) fn verify_local(file: &File) -> Result<(), Issue> {
        let mount = fs::fstatfs(file).map_err(|_| denied())?;
        let stat = fs::fstat(file).map_err(|_| denied())?;
        if mount.f_flags & libc::MNT_LOCAL as u32 == 0 || stat.st_flags & PROVIDER_FLAGS != 0 {
            return Err(denied());
        }
        Ok(())
    }
    fn refuse_remote_mount(path: &Path) -> Result<(), Issue> {
        // MNT_NOWAIT inspects existing kernel mount records without contacting
        // a remote server. Never stat/open a known remote path first.
        let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
        if !(1..=4096).contains(&count) {
            return Err(denied());
        }
        let capacity = count as usize + 32;
        let mut entries = Vec::<std::mem::MaybeUninit<libc::statfs>>::with_capacity(capacity);
        entries.resize_with(capacity, std::mem::MaybeUninit::uninit);
        let bytes = (capacity * std::mem::size_of::<libc::statfs>()) as libc::c_int;
        let filled = unsafe { libc::getfsstat(entries.as_mut_ptr().cast(), bytes, libc::MNT_NOWAIT) };
        if filled < 0 || filled as usize > capacity {
            return Err(denied());
        }
        for entry in &entries[..filled as usize] {
            // Only returned initialized entries are inspected.
            let entry = unsafe { entry.assume_init_ref() };
            let mount: Vec<u8> = entry.f_mntonname.iter().map(|c| *c as u8).take_while(|c| *c != 0).collect();
            if mount.len() == entry.f_mntonname.len() {
                return Err(denied());
            }
            if entry.f_flags & libc::MNT_LOCAL as u32 == 0 && path.starts_with(Path::new(std::ffi::OsStr::from_bytes(&mount))) {
                return Err(denied());
            }
        }
        Ok(())
    }
}
