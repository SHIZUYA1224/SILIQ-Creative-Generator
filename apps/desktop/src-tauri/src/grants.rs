//! Native-only grants. No command accepts a path from the WebView.
//! Inputs are read from checked, open file descriptors; outputs never overwrite.

use design_model::{Code, Issue};
#[cfg(unix)]
use std::{
    ffi::CString,
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

fn denied(message: &str) -> Issue {
    Issue::error(
        Code::PathDenied,
        message,
        "Choose a regular local file or directory in the native picker.",
    )
}

#[cfg(any(target_os = "macos", test))]
fn under_remote_mount(path: &Path, mounts: &[(PathBuf, bool)]) -> bool {
    mounts.iter().any(|(mount, local)| !local && path.starts_with(mount))
}

#[cfg(target_os = "macos")]
fn refuse_existing_remote_mount(path: &Path) -> Result<(), Issue> {
    // MNT_NOWAIT reads the kernel's existing mount table without refreshing a
    // remote filesystem. Do this before open/stat on any selected component.
    let count = unsafe { libc::getfsstat(std::ptr::null_mut(), 0, libc::MNT_NOWAIT) };
    if !(1..=4096).contains(&count) {
        return Err(denied("cannot inspect the existing local mount table"));
    }
    let capacity = count as usize + 32;
    let mut entries = Vec::<std::mem::MaybeUninit<libc::statfs>>::with_capacity(capacity);
    entries.resize_with(capacity, std::mem::MaybeUninit::uninit);
    let length = (capacity * std::mem::size_of::<libc::statfs>()) as libc::c_int;
    // SAFETY: the buffer holds length writable bytes; successful entries are
    // initialized by getfsstat and inspected only up to its returned count.
    let filled = unsafe { libc::getfsstat(entries.as_mut_ptr().cast(), length, libc::MNT_NOWAIT) };
    if filled < 0 || filled as usize > capacity {
        return Err(denied("cannot inspect the existing mount table safely"));
    }
    let mut mounts = Vec::new();
    for entry in &entries[..filled as usize] {
        let entry = unsafe { entry.assume_init_ref() };
        let bytes: Vec<u8> = entry.f_mntonname.iter().map(|c| *c as u8).take_while(|c| *c != 0).collect();
        if bytes.len() == entry.f_mntonname.len() {
            return Err(denied("invalid mount table entry"));
        }
        mounts.push((
            PathBuf::from(std::ffi::OsStr::from_bytes(&bytes)),
            entry.f_flags & libc::MNT_LOCAL as u32 != 0,
        ));
    }
    if under_remote_mount(path, &mounts) {
        return Err(denied(
            "existing network or automatic remote mounts are refused before selected paths are opened",
        ));
    }
    Ok(())
}

/// Refuse links in every path component. Darwin resolves the complete granted
/// target atomically with O_NOFOLLOW_ANY, without opening ungranted ancestors.
pub fn open_local(path: &Path, directory: bool) -> Result<File, Issue> {
    if !path.is_absolute() {
        return Err(denied("the native picker must return an absolute local path"));
    }
    if path.components().any(|c| !matches!(c, Component::RootDir | Component::Normal(_))) {
        return Err(denied("relative path components are refused"));
    }
    #[cfg(target_os = "macos")]
    {
        open_darwin_granted(path, directory)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let mut parent = File::open("/").map_err(|_| denied("cannot open filesystem root"))?;
        let parts: Vec<_> = path.components().filter(|c| !matches!(c, Component::RootDir)).collect();
        for (index, component) in parts.iter().enumerate() {
            let Component::Normal(name) = component else {
                return Err(denied("relative path components are refused"));
            };
            let name = CString::new(name.as_bytes()).map_err(|_| denied("NUL in selected path"))?;
            let is_dir = index + 1 != parts.len() || directory;
            #[cfg(target_os = "macos")]
            {
                // Inspect provider flags without opening content. An ordinary
                // read-open of a dataless file can itself request hydration.
                let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
                if unsafe { libc::fstatat(parent.as_raw_fd(), name.as_ptr(), status.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0 {
                    return Err(denied("cannot inspect selected path before opening it"));
                }
                let status = unsafe { status.assume_init() };
                if status.st_flags & (0x0000_0080 | 0x4000_0000) != 0 {
                    return Err(denied("cloud-only placeholders are refused before content is opened"));
                }
            }
            let flags = libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | if is_dir { libc::O_DIRECTORY } else { 0 };
            // SAFETY: parent is an owned live descriptor; name is NUL terminated.
            let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
            if fd < 0 {
                return Err(denied("selected path is inaccessible or contains a symbolic link"));
            }
            // SAFETY: openat returned a new owned descriptor.
            parent = unsafe { File::from_raw_fd(fd) };
            verify_local(&parent)?;
        }
        let meta = parent.metadata().map_err(|_| denied("cannot inspect selected file"))?;
        if directory && !meta.is_dir() || !directory && !meta.is_file() {
            return Err(denied("unexpected selected file type"));
        }
        if !directory && meta.nlink() != 1 {
            return Err(denied("hard-linked inputs are refused"));
        }
        Ok(parent)
    }
    #[cfg(not(unix))]
    {
        let _ = directory;
        Err(denied("local mount validation is not implemented on this platform"))
    }
}

#[cfg(target_os = "macos")]
fn open_darwin_granted(path: &Path, directory: bool) -> Result<File, Issue> {
    refuse_existing_remote_mount(path)?;
    let path_bytes = CString::new(path.as_os_str().as_bytes()).map_err(|_| denied("NUL in selected path"))?;
    let mut attributes = libc::attrlist {
        bitmapcount: libc::ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: libc::ATTR_CMN_FLAGS,
        volattr: 0,
        dirattr: 0,
        fileattr: 0,
        forkattr: 0,
    };
    // Metadata-only lookup avoids read-opening an offline provider item. The
    // kernel rejects ancestor links and grants access to the exact selected or
    // container path; no ancestor directory read permission is required.
    let mut flags = [0u32; 2];
    if unsafe {
        libc::getattrlist(
            path_bytes.as_ptr(),
            (&mut attributes as *mut libc::attrlist).cast(),
            flags.as_mut_ptr().cast(),
            std::mem::size_of_val(&flags),
            libc::FSOPT_NOFOLLOW | libc::FSOPT_NOFOLLOW_ANY,
        )
    } != 0
    {
        return Err(denied("cannot inspect the granted path without following links"));
    }
    if flags[0] != std::mem::size_of_val(&flags) as u32 {
        return Err(denied("invalid granted-path metadata"));
    }
    if flags[1] & (0x0000_0080 | 0x4000_0000) != 0 {
        return Err(denied("cloud-only placeholders are refused before content is opened"));
    }
    let options =
        libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW_ANY | libc::O_NONBLOCK | if directory { libc::O_DIRECTORY } else { 0 };
    // O_NOFOLLOW_ANY protects the entire lookup in one syscall, including
    // symlink swaps after the metadata check. O_NONBLOCK avoids opening a FIFO
    // indefinitely before the descriptor type is checked.
    let fd = unsafe { libc::open(path_bytes.as_ptr(), options) };
    if fd < 0 {
        return Err(denied("granted path is inaccessible or contains a symbolic link"));
    }
    let file = unsafe { File::from_raw_fd(fd) };
    verify_local(&file)?;
    let meta = file.metadata().map_err(|_| denied("cannot inspect granted file"))?;
    if directory && !meta.is_dir() || !directory && !meta.is_file() {
        return Err(denied("unexpected selected file type"));
    }
    if !directory && meta.nlink() != 1 {
        return Err(denied("hard-linked inputs are refused"));
    }
    let mut resolved = [0u8; libc::PATH_MAX as usize];
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETPATH, resolved.as_mut_ptr()) } != 0 {
        return Err(denied("cannot confirm the granted descriptor path"));
    }
    let length = resolved
        .iter()
        .position(|b| *b == 0)
        .ok_or_else(|| denied("invalid granted descriptor path"))?;
    if path.as_os_str().as_bytes() != &resolved[..length] {
        return Err(denied("selected path changed while the native grant was opened"));
    }
    Ok(file)
}

#[cfg(unix)]
pub(crate) fn verify_local(file: &File) -> Result<(), Issue> {
    #[cfg(target_os = "macos")]
    {
        let mut mount = std::mem::MaybeUninit::<libc::statfs>::uninit();
        // SAFETY: fstatfs initializes the output only on success.
        if unsafe { libc::fstatfs(file.as_raw_fd(), mount.as_mut_ptr()) } != 0 {
            return Err(denied("cannot inspect selected mount"));
        }
        let mount = unsafe { mount.assume_init() };
        if mount.f_flags & libc::MNT_LOCAL as u32 == 0 {
            return Err(denied("network mounts are refused"));
        }
        let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(file.as_raw_fd(), status.as_mut_ptr()) } != 0 {
            return Err(denied("cannot inspect selected file flags"));
        }
        let status = unsafe { status.assume_init() };
        // Darwin UF_OFFLINE and SF_DATALESS: never request cloud hydration.
        if status.st_flags & (0x0000_0080 | 0x4000_0000) != 0 {
            return Err(denied("cloud-only placeholders are refused"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        let mut mount = std::mem::MaybeUninit::<libc::statfs>::uninit();
        if unsafe { libc::fstatfs(file.as_raw_fd(), mount.as_mut_ptr()) } != 0 {
            return Err(denied("cannot inspect selected mount"));
        }
        let mount = unsafe { mount.assume_init() };
        if matches!(mount.f_type as u64, 0x6969 | 0xff53_4d42 | 0x517b | 0x6573_5546) {
            return Err(denied("network and FUSE mounts are refused"));
        }
    }
    Ok(())
}

pub fn read_selected(path: &Path, maximum: u64) -> Result<Vec<u8>, Issue> {
    let file = open_local(path, false)?;
    if file.metadata().map_err(|_| denied("cannot inspect selected size"))?.len() > maximum {
        return Err(Issue::error(
            Code::ResourceLimit,
            "selected input exceeds its size limit",
            "Choose a smaller file.",
        ));
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| denied("cannot read selected input"))?;
    if bytes.len() as u64 > maximum {
        return Err(Issue::error(
            Code::ResourceLimit,
            "selected input grew beyond its size limit",
            "Choose a smaller file.",
        ));
    }
    Ok(bytes)
}

pub struct DirectoryGrant {
    pub path: PathBuf,
    directory: File,
}

impl DirectoryGrant {
    pub fn new(path: PathBuf) -> Result<Self, Issue> {
        let directory = open_local(&path, true)?;
        Ok(Self { path, directory })
    }

    pub fn directory(&self) -> &File {
        &self.directory
    }

    /// Atomic app-owned metadata replacement. Public export commands use
    /// write_new; only fixed recovery/settings names use this internal path.
    pub fn replace_control(&self, name: &str, bytes: &[u8]) -> Result<(), Issue> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) || name.chars().any(char::is_control) {
            return Err(denied("invalid control file name"));
        }
        let temporary = crate::host::opaque_id("control");
        self.write_new(&temporary, bytes)?;
        #[cfg(unix)]
        {
            let temporary = CString::new(temporary).unwrap();
            let name = CString::new(name).unwrap();
            if unsafe {
                libc::renameat(
                    self.directory.as_raw_fd(),
                    temporary.as_ptr(),
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                )
            } != 0
            {
                unsafe {
                    libc::unlinkat(self.directory.as_raw_fd(), temporary.as_ptr(), 0);
                }
                return Err(denied("cannot commit the app-owned control file"));
            }
            self.directory.sync_all().map_err(|_| denied("cannot flush app-owned metadata"))?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            Err(denied("safe metadata replacement is not implemented on this platform"))
        }
    }

    pub fn write_new(&self, name: &str, bytes: &[u8]) -> Result<(), Issue> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) || name.chars().any(char::is_control) {
            return Err(denied("invalid output file name"));
        }
        #[cfg(unix)]
        {
            let name = CString::new(name).map_err(|_| denied("invalid output name"))?;
            // Use the granted descriptor directly, so renaming/replacing the
            // original directory path cannot redirect the write.
            let fd = unsafe {
                libc::openat(
                    self.directory.as_raw_fd(),
                    name.as_ptr(),
                    libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(Issue::error(
                    Code::OutputCollision,
                    "output could not be created without overwriting an existing file",
                    "Choose a fresh file name or output folder.",
                ));
            }
            let mut file = unsafe { File::from_raw_fd(fd) };
            if file.write_all(bytes).and_then(|_| file.sync_all()).is_err() {
                unsafe {
                    libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0);
                }
                return Err(Issue::error(
                    Code::SaveFailed,
                    "output write failed",
                    "Check free disk space and choose the output again.",
                ));
            }
            self.directory.sync_all().map_err(|_| denied("cannot flush output directory"))?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = bytes;
            Err(denied("safe output grants are not implemented on this platform"))
        }
    }
}

pub fn save_grant(path: &Path) -> Result<(DirectoryGrant, String), Issue> {
    let parent = path.parent().ok_or_else(|| denied("output has no directory"))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| denied("output name is not Unicode"))?
        .to_owned();
    Ok((DirectoryGrant::new(parent.to_owned())?, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn existing_remote_mount_classification_uses_component_boundaries() {
        let mounts = vec![
            (PathBuf::from("/"), true),
            (PathBuf::from("/Volumes/team"), false),
            (PathBuf::from("/Volumes/local"), true),
        ];
        assert!(under_remote_mount(Path::new("/Volumes/team/image.png"), &mounts));
        assert!(under_remote_mount(Path::new("/Volumes/team"), &mounts));
        assert!(!under_remote_mount(Path::new("/Volumes/team-local/image.png"), &mounts));
        assert!(!under_remote_mount(Path::new("/Volumes/local/image.png"), &mounts));
    }
    #[test]
    fn output_grant_refuses_traversal_and_overwrite() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("siliq-host-grant-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        let grant = DirectoryGrant::new(path.clone()).unwrap();
        assert!(grant.write_new("../escape.png", b"x").is_err());
        let _ = std::fs::remove_file(path.join("frame.png"));
        grant.write_new("frame.png", b"first").unwrap();
        assert!(grant.write_new("frame.png", b"second").is_err());
        assert_eq!(std::fs::read(path.join("frame.png")).unwrap(), b"first");
        std::fs::remove_dir_all(path).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn input_refuses_links() {
        let path = std::fs::canonicalize(std::env::temp_dir())
            .unwrap()
            .join(format!("siliq-host-links-{}", std::process::id()));
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("input"), b"x").unwrap();
        std::os::unix::fs::symlink(path.join("input"), path.join("link")).unwrap();
        assert!(read_selected(&path.join("link"), 10).is_err());
        std::fs::create_dir(path.join("nested")).unwrap();
        std::fs::write(path.join("nested/input"), b"nested").unwrap();
        std::os::unix::fs::symlink(path.join("nested"), path.join("ancestor-link")).unwrap();
        assert!(read_selected(&path.join("ancestor-link/input"), 10).is_err());
        std::fs::hard_link(path.join("input"), path.join("hard")).unwrap();
        assert!(read_selected(&path.join("input"), 10).is_err());
        let grant = DirectoryGrant::new(path.clone()).unwrap();
        grant.replace_control("link", b"new metadata").unwrap();
        assert_eq!(std::fs::read(path.join("input")).unwrap(), b"x");
        assert_eq!(std::fs::read(path.join("link")).unwrap(), b"new metadata");
        std::fs::remove_dir_all(path).unwrap();
    }
}
