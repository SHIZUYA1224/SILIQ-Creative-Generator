//! `siliq-render` — Phase 0 command-line renderer.
//!
//! ```text
//! siliq-render check  --doc <file.json>
//! siliq-render render --doc <file.json> --variant <id> --resources <dir>
//!                     --out-dir <dir> --name <file.png> [--report <file.json>]
//! siliq-render batch  --doc <template.json> --data <rows.csv> --variants <id,id>
//!                     --resources <dir> --out-dir <dir>
//!                     [--name-pattern "{rowId}_{variantId}.png"] [--manifest manifest.json]
//! ```
//!
//! `--resources` and `--out-dir` stand in for the user's read / write grants:
//! the engine reads only content-addressed files under the resource directory
//! and writes only new files directly inside the output directory.
//! Exit codes: 0 success, 1 usage error, 2 blocked by issues.

use design_model::{Code, Issue, load_document};
use design_render::render;
use design_render::resources::ResourceStore;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage:
  siliq-render check  --doc <file.json>
  siliq-render render --doc <file.json> --variant <id> --resources <dir> --out-dir <dir> --name <file.png> [--report <file.json>]
  siliq-render batch  --doc <template.json> --data <rows.csv> --variants <id,id> --resources <dir> --out-dir <dir> [--name-pattern \"{rowId}_{variantId}.png\"] [--manifest manifest.json]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((cmd, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(1);
    };
    let opts = match parse_opts(rest) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("error: {e}\n{USAGE}");
            return ExitCode::from(1);
        }
    };
    let result = match cmd.as_str() {
        "check" => cmd_check(&opts),
        "render" => cmd_render(&opts),
        "batch" => cmd_batch(&opts),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(1);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Usage(e)) => {
            eprintln!("error: {e}\n{USAGE}");
            ExitCode::from(1)
        }
        Err(Failure::Blocked(issues)) => {
            for i in &issues {
                eprintln!("{i}");
            }
            let json = serde_json::json!({ "status": "blocked", "issues": issues });
            println!("{}", serde_json::to_string_pretty(&json).expect("serializable"));
            ExitCode::from(2)
        }
    }
}

enum Failure {
    Usage(String),
    Blocked(Vec<Issue>),
}

impl From<Issue> for Failure {
    fn from(i: Issue) -> Self {
        Failure::Blocked(vec![i])
    }
}

fn parse_opts(rest: &[String]) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    let mut it = rest.iter();
    while let Some(k) = it.next() {
        let key = k.strip_prefix("--").ok_or_else(|| format!("unexpected argument {k:?}"))?;
        if ![
            "doc",
            "variant",
            "resources",
            "out-dir",
            "name",
            "report",
            "data",
            "variants",
            "name-pattern",
            "manifest",
        ]
        .contains(&key)
        {
            return Err(format!("unknown option --{key}"));
        }
        let v = it.next().ok_or_else(|| format!("--{key} needs a value"))?;
        if m.insert(key.to_owned(), v.clone()).is_some() {
            return Err(format!("--{key} given twice"));
        }
    }
    Ok(m)
}

fn req<'a>(o: &'a HashMap<String, String>, k: &str) -> Result<&'a str, Failure> {
    o.get(k)
        .map(String::as_str)
        .ok_or_else(|| Failure::Usage(format!("--{k} is required")))
}

fn read_document(path: &str) -> Result<Vec<u8>, Failure> {
    let p = Path::new(path);
    let meta = fs::symlink_metadata(p).map_err(|_| {
        Issue::error(
            Code::PathDenied,
            "document file does not exist",
            "Choose an existing document file.",
        )
    })?;
    if !meta.is_file() {
        return Err(Issue::error(
            Code::PathDenied,
            "document path is not a regular file (symlinks are refused)",
            "Choose a regular file.",
        )
        .into());
    }
    if meta.len() > design_model::validate::MAX_DOCUMENT_BYTES as u64 {
        return Err(Issue::error(Code::ResourceLimit, "document file is too large", "Reduce the document size.").into());
    }
    fs::read(p).map_err(|e| {
        Issue::error(
            Code::PathDenied,
            format!("document cannot be read: {}", e.kind()),
            "Check permissions.",
        )
        .into()
    })
}

fn cmd_check(o: &HashMap<String, String>) -> Result<(), Failure> {
    let bytes = read_document(req(o, "doc")?)?;
    let doc = load_document(&bytes).map_err(Failure::Blocked)?;
    let json = serde_json::json!({
        "status": "valid",
        "documentId": doc.document_id,
        "revision": doc.revision,
        "variants": doc.variants.iter().map(|v| serde_json::json!({"id": v.id, "approvalStatus": v.approval_status})).collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&json).expect("serializable"));
    Ok(())
}

fn cmd_render(o: &HashMap<String, String>) -> Result<(), Failure> {
    let doc_bytes = read_document(req(o, "doc")?)?;
    let variant = req(o, "variant")?;
    let store = ResourceStore::open(Path::new(req(o, "resources")?))?;
    let out_dir = open_output_dir(Path::new(req(o, "out-dir")?))?;
    let png_path = output_path(&out_dir, req(o, "name")?, "png")?;
    let report_path = o.get("report").map(|n| output_path(&out_dir, n, "json")).transpose()?;
    if report_path.as_ref() == Some(&png_path) {
        return Err(Issue::error(Code::OutputCollision, "image and report have the same name", "Use different names.").into());
    }

    let out = render(&doc_bytes, variant, &store).map_err(Failure::Blocked)?;

    write_new(&png_path, &out.png)?;
    let report_json = serde_json::to_string_pretty(&out.report).expect("serializable");
    if let Some(rp) = &report_path {
        write_new(rp, report_json.as_bytes())?;
    }
    println!("{report_json}");
    Ok(())
}

fn cmd_batch(o: &HashMap<String, String>) -> Result<(), Failure> {
    let template = read_document(req(o, "doc")?)?;
    let data = read_limited(req(o, "data")?, job_runner::csv::MAX_CSV_BYTES as u64, "data file")?;
    let variants: Vec<String> = req(o, "variants")?.split(',').map(str::to_owned).collect();
    let store = ResourceStore::open(Path::new(req(o, "resources")?))?;
    let out_dir = open_output_dir(Path::new(req(o, "out-dir")?))?;
    let pattern = o.get("name-pattern").map(String::as_str).unwrap_or("{rowId}_{variantId}.png");
    let manifest_name = o.get("manifest").map(String::as_str).unwrap_or("manifest.json");
    let started = std::time::Instant::now();
    let manifest = job_runner::run_batch(&job_runner::BatchRequest {
        template: &template,
        data: &data,
        variants: &variants,
        name_pattern: pattern,
        manifest_name,
        store: &store,
        out_dir: &out_dir,
        options: Default::default(),
    })
    .map_err(Failure::Blocked)?;
    let json = serde_json::json!({
        "status": manifest.status,
        "rows": manifest.rows,
        "variants": manifest.variants,
        "outputs": manifest.total,
        "manifest": manifest_name,
        "seconds": (started.elapsed().as_secs_f64() * 100.0).round() / 100.0,
    });
    println!("{}", serde_json::to_string_pretty(&json).expect("serializable"));
    Ok(())
}

fn read_limited(path: &str, max: u64, what: &str) -> Result<Vec<u8>, Failure> {
    let p = Path::new(path);
    let meta = fs::symlink_metadata(p)
        .map_err(|_| Issue::error(Code::PathDenied, format!("{what} does not exist"), "Choose an existing file."))?;
    if !meta.is_file() {
        return Err(Issue::error(
            Code::PathDenied,
            format!("{what} is not a regular file (symlinks are refused)"),
            "Choose a regular file.",
        )
        .into());
    }
    if meta.len() > max {
        return Err(Issue::error(Code::ResourceLimit, format!("{what} is too large"), "Use a smaller file.").into());
    }
    fs::read(p).map_err(|e| {
        Issue::error(
            Code::PathDenied,
            format!("{what} cannot be read: {}", e.kind()),
            "Check permissions.",
        )
        .into()
    })
}

fn open_output_dir(dir: &Path) -> Result<PathBuf, Failure> {
    let meta = fs::symlink_metadata(dir).map_err(|_| {
        Issue::error(
            Code::PathDenied,
            "output directory does not exist",
            "Create or choose an output directory.",
        )
    })?;
    if !meta.is_dir() {
        return Err(Issue::error(
            Code::PathDenied,
            "output path is not a directory (symlinks are refused)",
            "Choose a real directory.",
        )
        .into());
    }
    fs::canonicalize(dir).map_err(|e| {
        Issue::error(
            Code::PathDenied,
            format!("cannot resolve output directory: {}", e.kind()),
            "Choose an accessible directory.",
        )
        .into()
    })
}

/// Output names are plain file names inside the granted directory.
fn output_path(dir: &Path, name: &str, ext: &str) -> Result<PathBuf, Failure> {
    job_runner::naming::validate_output_name(name, ext)?;
    let path = dir.join(name);
    if fs::symlink_metadata(&path).is_ok() {
        return Err(Issue::error(
            Code::OutputCollision,
            format!("{name} already exists in the output directory"),
            "Choose a new name or a new output directory; existing files are never overwritten.",
        )
        .into());
    }
    Ok(path)
}

/// Write to a temporary file, flush it, then publish it under the final name
/// with `hard_link`, which fails instead of replacing an existing file.
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    let dir = path.parent().expect("output path has a parent");
    let name = path.file_name().expect("output path has a name").to_string_lossy();
    let tmp = dir.join(format!(".{name}.partial-{}", std::process::id()));
    let fail = |what: &str, e: std::io::Error| -> Failure {
        Issue::error(
            Code::PathDenied,
            format!("{what}: {}", e.kind()),
            "Check that the output directory is writable and has free space.",
        )
        .into()
    };
    let result = (|| {
        let mut f = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| fail("cannot create temporary file", e))?;
        f.write_all(bytes).map_err(|e| fail("write failed", e))?;
        f.sync_all().map_err(|e| fail("flush failed", e))?;
        drop(f);
        fs::hard_link(&tmp, path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Failure::from(Issue::error(
                    Code::OutputCollision,
                    format!("{name} appeared while rendering"),
                    "Choose a new name; existing files are never overwritten.",
                ))
            } else {
                fail("cannot publish output", e)
            }
        })
    })();
    let _ = fs::remove_file(&tmp);
    result
}
