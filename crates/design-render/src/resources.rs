//! Content-addressed resource store, laid out like the `.siliqcg` package:
//!
//! ```text
//! <root>/assets/<sha256>.<png|jpg|webp|svg>
//! <root>/fonts/<sha256>.<otf|ttf|ttc>
//! ```
//!
//! File names are built only from a validated lowercase SHA-256 and a fixed
//! extension, so a document cannot name an arbitrary path. Symlinks are
//! refused, sizes are capped before reading, and bytes are re-hashed.

use design_model::{Code, Issue, validate::is_sha256_hex};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const MAX_IMAGE_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ResourceStore {
    root: PathBuf,
    /// Verified bytes by `<sub>/<sha>.<ext>`. A store instance therefore acts
    /// as a snapshot: once a file has been read and verified, later changes
    /// on disk do not reach renders made with this instance (BAT-003).
    verified: Arc<Mutex<HashMap<String, Arc<Vec<u8>>>>>,
}

/// Why a resource could not be read. Converted to an [`Issue`] by callers
/// that know the node / asset context.
#[derive(Debug)]
pub enum ReadError {
    Missing,
    Denied(String),
    TooLarge(u64),
    Checksum,
    Io(String),
}

impl ReadError {
    pub fn into_issue(self, what: &str, missing_code: Code) -> Issue {
        match self {
            ReadError::Missing => Issue::error(
                missing_code,
                format!("{what} file is not present in the resource store"),
                "Place the file named by its SHA-256 in the resource directory.",
            ),
            ReadError::Denied(why) => Issue::error(
                Code::PathDenied,
                format!("{what}: {why}"),
                "Use a regular file inside the granted resource directory.",
            ),
            ReadError::TooLarge(n) => Issue::error(
                Code::ResourceLimit,
                format!("{what} is {n} bytes, above the limit"),
                "Use a smaller file.",
            ),
            ReadError::Checksum => Issue::error(
                Code::AssetChecksum,
                format!("{what} bytes do not match the recorded SHA-256"),
                "Re-import the file or correct the recorded hash.",
            ),
            ReadError::Io(e) => Issue::error(
                missing_code,
                format!("{what} could not be read: {e}"),
                "Check the file permissions.",
            ),
        }
    }
}

impl ResourceStore {
    /// `dir` is the directory the user granted for reading resources.
    pub fn open(dir: &Path) -> Result<Self, Issue> {
        let meta = fs::symlink_metadata(dir).map_err(|_| {
            Issue::error(
                Code::PathDenied,
                "resource directory does not exist",
                "Choose an existing directory.",
            )
        })?;
        if !meta.is_dir() {
            return Err(Issue::error(
                Code::PathDenied,
                "resource path is not a directory (symlinks are refused)",
                "Choose a real directory.",
            ));
        }
        let root = fs::canonicalize(dir).map_err(|e| {
            Issue::error(
                Code::PathDenied,
                format!("cannot resolve resource directory: {e}"),
                "Choose an accessible directory.",
            )
        })?;
        Ok(ResourceStore {
            root,
            verified: Arc::default(),
        })
    }

    pub fn snapshot_size(&self) -> u64 {
        self.verified.lock().expect("cache lock").values().map(|b| b.len() as u64).sum()
    }

    /// Read `<sub>/<sha>.<ext>` for the first extension that exists.
    pub fn read(&self, sub: &str, sha: &str, exts: &[&str], max: u64) -> Result<(Arc<Vec<u8>>, String), ReadError> {
        if !is_sha256_hex(sha) {
            return Err(ReadError::Denied("invalid content hash".into()));
        }
        {
            let cache = self.verified.lock().expect("cache lock");
            for ext in exts {
                if let Some(b) = cache.get(&format!("{sub}/{sha}.{ext}")) {
                    return Ok((b.clone(), (*ext).to_owned()));
                }
            }
        }
        let dir = self.root.join(sub);
        match fs::symlink_metadata(&dir) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => return Err(ReadError::Denied(format!("{sub}/ is not a real directory"))),
            Err(_) => return Err(ReadError::Missing),
        }
        let mut found = Vec::new();
        for ext in exts {
            let p = dir.join(format!("{sha}.{ext}"));
            if fs::symlink_metadata(&p).is_ok() {
                found.push((p, *ext));
            }
        }
        let (path, ext) = match found.len() {
            0 => return Err(ReadError::Missing),
            1 => found.pop().unwrap(),
            _ => return Err(ReadError::Denied("more than one file with the same hash".into())),
        };
        let meta = fs::symlink_metadata(&path).map_err(|e| ReadError::Io(e.kind().to_string()))?;
        if meta.file_type().is_symlink() {
            return Err(ReadError::Denied("symbolic links are refused".into()));
        }
        if !meta.is_file() {
            return Err(ReadError::Denied("not a regular file".into()));
        }
        if meta.len() > max || self.snapshot_size().saturating_add(meta.len()) > 512 * 1024 * 1024 {
            return Err(ReadError::TooLarge(meta.len()));
        }
        // Defence in depth: the resolved path must stay under the root.
        let canon = fs::canonicalize(&path).map_err(|e| ReadError::Io(e.kind().to_string()))?;
        if !canon.starts_with(&self.root) {
            return Err(ReadError::Denied("resolved outside the resource directory".into()));
        }
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        fs::File::open(&canon)
            .and_then(|f| f.take(max + 1).read_to_end(&mut bytes))
            .map_err(|e| ReadError::Io(e.kind().to_string()))?;
        if bytes.len() as u64 > max {
            return Err(ReadError::TooLarge(bytes.len() as u64));
        }
        if sha256_hex(&bytes) != sha {
            return Err(ReadError::Checksum);
        }
        let bytes = Arc::new(bytes);
        let mut cache = self.verified.lock().expect("cache lock");
        let retained: u64 = cache.values().map(|b| b.len() as u64).sum();
        if retained.saturating_add(bytes.len() as u64) > 512 * 1024 * 1024 {
            return Err(ReadError::TooLarge(retained + bytes.len() as u64));
        }
        cache.insert(format!("{sub}/{sha}.{ext}"), bytes.clone());
        Ok((bytes, ext.to_owned()))
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest.iter() {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Deterministic, authored ICC v2 matrix/TRC sRGB profile (no external file access).
pub const SRGB_ICC: &[u8] = include_bytes!("../resources/srgb.icc");
