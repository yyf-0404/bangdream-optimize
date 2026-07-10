use std::{env, fs, path::PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let metadata_path = manifest_dir.join("../../apps/web/package.json");
    println!("cargo:rerun-if-changed={}", metadata_path.display());

    let metadata = fs::read_to_string(&metadata_path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", metadata_path.display()));
    let metadata: serde_json::Value = serde_json::from_str(&metadata)
        .unwrap_or_else(|error| panic!("failed to parse {}: {error}", metadata_path.display()));
    let version = metadata
        .get("version")
        .and_then(serde_json::Value::as_str)
        .filter(|version| !version.is_empty())
        .unwrap_or_else(|| panic!("{} has no version", metadata_path.display()));
    println!("cargo:rustc-env=BANGDREAM_OPTIMIZE_APP_VERSION={version}");
}
