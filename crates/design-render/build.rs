use sha2::{Digest, Sha256};
use std::{env, fs, path::PathBuf};
fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let mut hash = Sha256::new();
    let paths = [
        "Cargo.lock",
        "rust-toolchain.toml",
        "third_party/skia-binaries.lock",
        "schemas/design.core.schema.json",
        "crates/design-render/Cargo.toml",
        "crates/design-render/build.rs",
        "crates/design-render/src/lib.rs",
        "crates/design-render/src/text.rs",
        "crates/design-render/src/raster.rs",
        "crates/design-render/src/resources.rs",
        "crates/design-render/resources/srgb.icc",
        "crates/design-model/src/types.rs",
        "crates/design-model/src/validate.rs",
        "crates/design-model/src/variant.rs",
        "crates/design-model/src/binding.rs",
        "crates/design-svg/src/lib.rs",
        "crates/design-svg/src/parse.rs",
        "crates/design-svg/src/path.rs",
        "crates/design-svg/src/transform.rs",
        "crates/design-export/src/lib.rs",
        "crates/design-export/Cargo.toml",
        "crates/job-runner/src/lib.rs",
        "crates/job-runner/src/controlled.rs",
        "crates/job-runner/src/csv.rs",
        "crates/job-runner/src/naming.rs",
        "crates/job-runner/src/io.rs",
    ];
    for relative in paths {
        let path = root.join(relative);
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes = fs::read(&path).expect("render fingerprint source exists");
        hash.update(relative.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    for name in [
        "TARGET",
        "CARGO_CFG_TARGET_FEATURE",
        "CARGO_ENCODED_RUSTFLAGS",
        "OPT_LEVEL",
        "PROFILE",
    ] {
        hash.update(name.as_bytes());
        hash.update(env::var(name).unwrap_or_default().as_bytes());
        println!("cargo:rerun-if-env-changed={name}");
    }
    let fingerprint: String = hash.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    println!("cargo:rustc-env=SILIQ_RENDER_BUILD_FINGERPRINT={fingerprint}");
}
