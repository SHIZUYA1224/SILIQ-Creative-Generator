//! CLI behaviour: grants, output naming, no-overwrite, exit codes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/public-safe");

fn doc() -> String {
    format!("{FIXTURES}/documents/basic.json")
}

fn resources() -> String {
    format!("{FIXTURES}/resources")
}

fn temp_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("cli-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_siliq-render")).args(args).output().unwrap()
}

fn render(out_dir: &Path, name: &str) -> Output {
    cli(&[
        "render",
        "--doc",
        &doc(),
        "--variant",
        "square",
        "--resources",
        &resources(),
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--name",
        name,
    ])
}

fn stdout_code(o: &Output) -> String {
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    v["issues"][0]["code"].as_str().unwrap().to_owned()
}

fn entries(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn renders_png_and_report_and_nothing_else() {
    let d = temp_dir();
    let o = cli(&[
        "render",
        "--doc",
        &doc(),
        "--variant",
        "square",
        "--resources",
        &resources(),
        "--out-dir",
        d.to_str().unwrap(),
        "--name",
        "a.png",
        "--report",
        "a.json",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(entries(&d), vec!["a.json", "a.png"]);
    let png = fs::read(d.join("a.png")).unwrap();
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let report: serde_json::Value = serde_json::from_slice(&fs::read(d.join("a.json")).unwrap()).unwrap();
    assert_eq!(report["outputSha256"].as_str().unwrap(), design_render::resources::sha256_hex(&png));
}

#[test]
fn existing_output_is_never_overwritten() {
    let d = temp_dir();
    fs::write(d.join("a.png"), b"keep me").unwrap();
    let o = render(&d, "a.png");
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(stdout_code(&o), "E_OUTPUT_COLLISION");
    assert_eq!(fs::read(d.join("a.png")).unwrap(), b"keep me");
    assert_eq!(entries(&d), vec!["a.png"]);
}

// A NUL byte cannot be passed through argv at all, so it is not listed.
#[test]
fn output_names_are_confined_to_the_granted_directory() {
    let d = temp_dir();
    for name in [
        "../escape.png",
        "/tmp/abs.png",
        "sub/a.png",
        ".hidden.png",
        "a..png",
        "CON.png",
        "nul.png",
        "a.PNG",
        "a.jpg",
        "a b.png",
        "名前.png",
        "",
    ] {
        let o = render(&d, name);
        assert_eq!(o.status.code(), Some(2), "{name:?}: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(stdout_code(&o), "E_PATH_DENIED", "{name:?}");
    }
    assert!(entries(&d).is_empty());
    assert!(!d.parent().unwrap().join("escape.png").exists());
}

#[cfg(unix)]
#[test]
fn symlinked_output_dir_and_document_are_refused() {
    let d = temp_dir();
    let real = d.join("real");
    fs::create_dir(&real).unwrap();
    let link = d.join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let o = render(&link, "a.png");
    assert_eq!(stdout_code(&o), "E_PATH_DENIED");
    assert!(entries(&real).is_empty());

    let doc_link = d.join("doc.json");
    std::os::unix::fs::symlink(doc(), &doc_link).unwrap();
    let o = cli(&[
        "render",
        "--doc",
        doc_link.to_str().unwrap(),
        "--variant",
        "square",
        "--resources",
        &resources(),
        "--out-dir",
        real.to_str().unwrap(),
        "--name",
        "a.png",
    ]);
    assert_eq!(stdout_code(&o), "E_PATH_DENIED");
}

#[test]
fn blocked_render_writes_nothing() {
    let d = temp_dir();
    let o = cli(&[
        "render",
        "--doc",
        &doc(),
        "--variant",
        "wide-draft",
        "--resources",
        &resources(),
        "--out-dir",
        d.to_str().unwrap(),
        "--name",
        "a.png",
    ]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(stdout_code(&o), "E_VARIANT_NOT_APPROVED");
    assert!(entries(&d).is_empty());
}

#[test]
fn check_validates_without_rendering() {
    let o = cli(&["check", "--doc", &doc()]);
    assert!(o.status.success());
    let sample = format!("{}/../../examples/design.core.sample.json", env!("CARGO_MANIFEST_DIR"));
    assert!(cli(&["check", "--doc", &sample]).status.success());
}

#[test]
fn usage_errors_exit_with_1() {
    assert_eq!(cli(&[]).status.code(), Some(1));
    assert_eq!(cli(&["render", "--doc"]).status.code(), Some(1));
    assert_eq!(cli(&["render", "--bogus", "x"]).status.code(), Some(1));
    assert_eq!(cli(&["render", "--doc", "a", "--doc", "b"]).status.code(), Some(1));
}
