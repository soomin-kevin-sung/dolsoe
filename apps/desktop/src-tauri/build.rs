use std::{env, fs, path::Path};

const PLACEHOLDER_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn require_release_runtime_resources() {
    if env::var("PROFILE").as_deref() != Ok("release") {
        return;
    }

    for path in [
        "resources/runtime-packs/cpu.zip",
        "resources/runtime-packs/cpu-index.json",
    ] {
        if !Path::new(path).is_file() {
            panic!("release packaging requires bundled runtime resource: {path}");
        }
    }

    let source_path = "resources/runtime-source.default.json";
    let source_bytes = fs::read(source_path)
        .unwrap_or_else(|error| panic!("failed to read {source_path}: {error}"));
    let source: serde_json::Value = serde_json::from_slice(&source_bytes)
        .unwrap_or_else(|error| panic!("failed to parse {source_path}: {error}"));
    let digest = source
        .get("manifestSha256")
        .and_then(|value| value.as_str())
        .unwrap_or_else(|| panic!("{source_path} is missing manifestSha256"));
    if digest == PLACEHOLDER_DIGEST {
        panic!("release packaging refuses the placeholder runtime source digest");
    }
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|value| value.is_ascii_digit() || (b'a'..=b'f').contains(&value))
    {
        panic!("release packaging requires a lowercase SHA-256 manifest digest");
    }
}

fn main() {
    require_release_runtime_resources();
    tauri_build::build()
}
