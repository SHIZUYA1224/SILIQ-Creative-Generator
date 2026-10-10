//! Batch production against the public-safe retail-promo template.

use design_model::Code;
use design_render::resources::{ResourceStore, sha256_hex};
use job_runner::{BatchRequest, Manifest, run_batch};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/public-safe");

fn template() -> Vec<u8> {
    fs::read(format!("{FIXTURES}/documents/retail-promo-template.json")).unwrap()
}

fn stores_csv() -> Vec<u8> {
    fs::read(format!("{FIXTURES}/data/retail-promo-stores.csv")).unwrap()
}

fn store() -> ResourceStore {
    ResourceStore::open(Path::new(&format!("{FIXTURES}/resources"))).unwrap()
}

fn out_dir() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("batch-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::canonicalize(d).unwrap()
}

fn run(data: &[u8], variants: &[&str], dir: &Path) -> Result<Manifest, Vec<design_model::Issue>> {
    let variants: Vec<String> = variants.iter().map(|s| s.to_string()).collect();
    run_batch(&BatchRequest {
        template: &template(),
        data,
        variants: &variants,
        name_pattern: "{rowId}_{variantId}.png",
        manifest_name: "manifest.json",
        store: &store(),
        out_dir: dir,
        options: Default::default(),
    })
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
fn ten_rows_two_variants_produce_twenty_files_and_a_manifest() {
    let dir = out_dir();
    let m = run(&stores_csv(), &["portrait", "square"], &dir).unwrap();
    assert_eq!((m.rows, m.total, m.items.len()), (10, 20, 20));
    let files = entries(&dir);
    assert_eq!(files.len(), 21, "{files:?}");
    assert!(files.contains(&"store-001_portrait.png".to_string()));
    assert!(files.contains(&"manifest.json".to_string()));
    for item in &m.items {
        let bytes = fs::read(dir.join(&item.file)).unwrap();
        assert_eq!(sha256_hex(&bytes), item.sha256, "{}", item.file);
        assert!(item.warnings.is_empty());
    }
    // 6-digit prices use an approved smaller size in portrait; others keep 250.
    let size = |row: &str, v: &str| {
        m.items
            .iter()
            .find(|i| i.row_id == row && i.variant_id == v)
            .unwrap()
            .applied_font_sizes["price"]
    };
    assert_eq!(size("store-001", "portrait"), 250.0);
    assert_eq!(size("store-005", "portrait"), 230.0);
    assert_eq!(size("store-005", "square"), 210.0);
}

#[test]
fn same_input_gives_identical_outputs() {
    let (a, b) = (out_dir(), out_dir());
    let ma = run(&stores_csv(), &["portrait"], &a).unwrap();
    let mb = run(&stores_csv(), &["portrait"], &b).unwrap();
    let ha: Vec<_> = ma.items.iter().map(|i| (&i.file, &i.sha256, &i.input_digest)).collect();
    let hb: Vec<_> = mb.items.iter().map(|i| (&i.file, &i.sha256, &i.input_digest)).collect();
    assert_eq!(ha, hb);
    assert_eq!(
        fs::read(a.join("manifest.json")).unwrap(),
        fs::read(b.join("manifest.json")).unwrap()
    );
}

#[test]
fn one_bad_row_blocks_everything() {
    let mut csv = String::from_utf8(stores_csv()).unwrap();
    csv.push_str("store-011,タイ語ภาษา店,\"9,800\",45分,0120-000-111,受付\n");
    csv.push_str("store-012,長すぎる価格店,\"123,456,789\",45分,0120-000-112,受付\n");
    csv.push_str("store-013,電話なし店,\"9,800\",45分,,受付\n");
    let dir = out_dir();
    let issues = run(csv.as_bytes(), &["portrait", "square"], &dir).unwrap_err();
    let rows: Vec<(&str, Code)> = issues
        .iter()
        .map(|i| (i.location.row_id.as_deref().unwrap_or(""), i.code))
        .collect();
    assert!(rows.contains(&("store-011", Code::GlyphMissing)), "{rows:?}");
    assert!(rows.contains(&("store-012", Code::DataInvalid)), "{rows:?}");
    assert!(rows.contains(&("store-013", Code::DataInvalid)), "{rows:?}");
    // Issues carry the variant and node as well.
    let glyph = issues.iter().find(|i| i.code == Code::GlyphMissing).unwrap();
    assert_eq!(glyph.location.node_id.as_deref(), Some("shop-sub"));
    assert!(glyph.location.variant_id.is_some());
    // Nothing at all was written, not even for the good rows.
    assert!(entries(&dir).is_empty(), "{:?}", entries(&dir));
}

#[test]
fn unknown_or_missing_columns_are_refused() {
    let dir = out_dir();
    let e = run(
        b"rowId,branch,price,fastest,tel,hours,colour\nr1,a,1,1,1,1,red\n",
        &["portrait"],
        &dir,
    )
    .unwrap_err();
    assert!(e.iter().any(|i| i.code == Code::DataInvalid && i.message.contains("colour")));
    let e = run(b"rowId,branch\nr1,a\n", &["portrait"], &dir).unwrap_err();
    assert!(e.iter().filter(|i| i.message.contains("has no column")).count() >= 4);
    let e = run(b"branch,price\na,1\n", &["portrait"], &dir).unwrap_err();
    assert!(e[0].message.contains("rowId"));
    assert!(entries(&dir).is_empty());
}

#[test]
fn duplicate_and_unsafe_row_ids_are_refused() {
    let dir = out_dir();
    let header = "rowId,branch,price,fastest,tel,hours\n";
    let e = run(format!("{header}a,x,1,1,1,1\na,y,1,1,1,1\n").as_bytes(), &["portrait"], &dir).unwrap_err();
    assert!(e.iter().any(|i| i.message.contains("duplicate rowId")));
    let e = run(format!("{header}../x,x,1,1,1,1\n").as_bytes(), &["portrait"], &dir).unwrap_err();
    assert!(e.iter().any(|i| i.message.contains("rowId")));
    assert!(entries(&dir).is_empty());
}

#[test]
fn existing_outputs_are_never_overwritten() {
    let dir = out_dir();
    fs::write(dir.join("store-003_square.png"), b"keep").unwrap();
    let e = run(&stores_csv(), &["portrait", "square"], &dir).unwrap_err();
    assert!(
        e.iter()
            .any(|i| i.code == Code::OutputCollision && i.location.row_id.as_deref() == Some("store-003"))
    );
    assert_eq!(entries(&dir), vec!["store-003_square.png"]);
    assert_eq!(fs::read(dir.join("store-003_square.png")).unwrap(), b"keep");
}

#[test]
fn unknown_draft_or_repeated_variants_are_refused() {
    let dir = out_dir();
    assert_eq!(run(&stores_csv(), &["landscape"], &dir).unwrap_err()[0].code, Code::LayoutUndefined);
    assert_eq!(
        run(&stores_csv(), &["portrait", "portrait"], &dir).unwrap_err()[0].code,
        Code::LayoutUndefined
    );
    assert!(entries(&dir).is_empty());
}

#[test]
fn name_patterns_are_restricted() {
    let dir = out_dir();
    let variants = vec!["portrait".to_string()];
    let req = |pattern: &'static str| BatchRequest {
        template: Box::leak(template().into_boxed_slice()),
        data: Box::leak(stores_csv().into_boxed_slice()),
        variants: Box::leak(variants.clone().into_boxed_slice()),
        name_pattern: pattern,
        manifest_name: "manifest.json",
        store: Box::leak(Box::new(store())),
        out_dir: Box::leak(dir.clone().into_boxed_path()),
        options: Default::default(),
    };
    // Unknown placeholder, path separator, and a pattern that collides.
    assert_eq!(run_batch(&req("{rowId}_{price}.png")).unwrap_err()[0].code, Code::PathDenied);
    assert_eq!(run_batch(&req("../{rowId}.png")).unwrap_err()[0].code, Code::PathDenied);
    assert!(
        run_batch(&req("same.png"))
            .unwrap_err()
            .iter()
            .any(|i| i.code == Code::OutputCollision)
    );
    assert!(entries(&dir).is_empty());
}
