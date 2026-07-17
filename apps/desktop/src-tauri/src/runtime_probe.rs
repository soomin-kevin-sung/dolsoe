use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::Manager;

const MAX_RUNTIME_PACK_ID_LEN: usize = 64;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfoDto {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub runtime_version: String,
    pub llama_cpp_commit: String,
    pub max_parallel_slots: u32,
}

fn validate_runtime_pack_id(runtime_pack_id: &str) -> Result<(), String> {
    if runtime_pack_id.is_empty() {
        return Err("runtime pack ID must not be empty".into());
    }
    if runtime_pack_id.len() > MAX_RUNTIME_PACK_ID_LEN {
        return Err(format!(
            "runtime pack ID must not exceed {MAX_RUNTIME_PACK_ID_LEN} ASCII characters"
        ));
    }
    if runtime_pack_id == "." || runtime_pack_id.contains("..") {
        return Err("runtime pack ID must not contain traversal components".into());
    }
    if !runtime_pack_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(
            "runtime pack ID may contain only ASCII letters, digits, dots, underscores, and hyphens"
                .into(),
        );
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn runtime_library_filename() -> &'static str {
    "local_llm_runtime.dll"
}

#[cfg(target_os = "macos")]
fn runtime_library_filename() -> &'static str {
    "liblocal_llm_runtime.dylib"
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn runtime_library_filename() -> &'static str {
    "liblocal_llm_runtime.so"
}

fn resolve_runtime_library(runtime_root: &Path, runtime_pack_id: &str) -> Result<PathBuf, String> {
    validate_runtime_pack_id(runtime_pack_id)?;

    if !runtime_root.is_dir() {
        return Err(format!(
            "trusted runtime root does not exist: {}",
            runtime_root.display()
        ));
    }
    let pack_dir = runtime_root.join(runtime_pack_id);
    if !pack_dir.is_dir() {
        return Err(format!(
            "runtime pack does not exist: {}",
            pack_dir.display()
        ));
    }
    let candidate = pack_dir.join(runtime_library_filename());
    if !candidate.is_file() {
        return Err(format!(
            "runtime library does not exist: {}",
            candidate.display()
        ));
    }

    let canonical_root = runtime_root
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize trusted runtime root: {error}"))?;
    let canonical_candidate = candidate
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize runtime library: {error}"))?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err("runtime library escapes trusted runtime root".into());
    }
    Ok(canonical_candidate)
}

fn runtime_info_dto(info: &llm_runtime::RuntimeInfo) -> RuntimeInfoDto {
    RuntimeInfoDto {
        abi_major: info.abi_major,
        abi_minor: info.abi_minor,
        runtime_version: info.runtime_version.clone(),
        llama_cpp_commit: info.llama_cpp_commit.clone(),
        max_parallel_slots: info.capabilities.max_parallel_slots,
    }
}

#[tauri::command]
pub async fn probe_runtime(
    app: tauri::AppHandle,
    runtime_pack_id: String,
) -> Result<RuntimeInfoDto, String> {
    let runtime_root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("runtime-packs");

    tauri::async_runtime::spawn_blocking(move || {
        let path = resolve_runtime_library(&runtime_root, &runtime_pack_id)?;
        // The backend/runtime installer exclusively owns writes to this trusted root.
        // SAFETY: `path` is the canonical path of a project-managed, ABI-conforming runtime pack.
        let runtime = unsafe { llm_runtime::RuntimeLibrary::load(&path) }
            .map_err(|error| error.to_string())?;
        Ok(runtime_info_dto(runtime.info()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use llm_runtime::{Capabilities, RuntimeInfo};

    use super::*;

    static NEXT_TEMP_DIR_ID: AtomicU64 = AtomicU64::new(0);

    struct TestDir {
        path: PathBuf,
    }

    impl TestDir {
        fn new() -> Self {
            let unique = format!(
                "local-llm-wiki-runtime-probe-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("system time must follow Unix epoch")
                    .as_nanos(),
                NEXT_TEMP_DIR_ID.fetch_add(1, Ordering::Relaxed)
            );
            let path = std::env::temp_dir().join(unique);
            fs::create_dir(&path).expect("create unique test directory");
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let is_owned_test_dir = self.path.starts_with(std::env::temp_dir())
                && self.path.file_name().is_some_and(|name| {
                    name.to_string_lossy()
                        .starts_with("local-llm-wiki-runtime-probe-")
                });
            assert!(is_owned_test_dir, "refusing to clean unexpected test path");
            if let Err(error) = fs::remove_dir_all(&self.path) {
                assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
            }
        }
    }

    #[test]
    fn accepts_release_runtime_pack_ids() {
        for runtime_pack_id in ["stable", "llama.cpp-1_2.3", "CUDA12", "a"] {
            assert!(validate_runtime_pack_id(runtime_pack_id).is_ok());
        }
    }

    #[test]
    fn rejects_invalid_runtime_pack_ids() {
        let overlong = "a".repeat(MAX_RUNTIME_PACK_ID_LEN + 1);
        for runtime_pack_id in [
            "",
            ".",
            "..",
            "../escape",
            "pack/child",
            "pack\\child",
            "pack name",
            "runtimé",
            &overlong,
        ] {
            assert!(
                validate_runtime_pack_id(runtime_pack_id).is_err(),
                "accepted invalid runtime pack ID: {runtime_pack_id:?}"
            );
        }
    }

    #[test]
    fn resolves_existing_runtime_library_under_trusted_root() {
        let temp = TestDir::new();
        let runtime_root = temp.path().join("runtime-packs");
        let pack_dir = runtime_root.join("stable-1.0");
        fs::create_dir_all(&pack_dir).expect("create runtime pack");
        let library = pack_dir.join(runtime_library_filename());
        fs::write(&library, b"not a real runtime").expect("create placeholder runtime library");

        let resolved = resolve_runtime_library(&runtime_root, "stable-1.0")
            .expect("resolve trusted runtime library");

        assert_eq!(resolved, library.canonicalize().unwrap());
    }

    #[test]
    fn resolver_reports_missing_root_pack_and_library() {
        let temp = TestDir::new();
        let runtime_root = temp.path().join("runtime-packs");

        let missing_root = resolve_runtime_library(&runtime_root, "stable").unwrap_err();
        assert!(missing_root.contains("root does not exist"));

        fs::create_dir(&runtime_root).expect("create runtime root");
        let missing_pack = resolve_runtime_library(&runtime_root, "stable").unwrap_err();
        assert!(missing_pack.contains("pack does not exist"));

        fs::create_dir(runtime_root.join("stable")).expect("create runtime pack");
        let missing_library = resolve_runtime_library(&runtime_root, "stable").unwrap_err();
        assert!(missing_library.contains("library does not exist"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_runtime_pack_junction_that_escapes_trusted_root() {
        use std::process::Command;

        let temp = TestDir::new();
        let runtime_root = temp.path().join("runtime-packs");
        let pack_dir = runtime_root.join("stable");
        let outside_pack = temp.path().join("outside-pack");
        fs::create_dir(&runtime_root).expect("create runtime root");
        fs::create_dir(&outside_pack).expect("create outside pack");
        fs::write(
            outside_pack.join(runtime_library_filename()),
            b"not a real runtime",
        )
        .expect("create outside library");
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&pack_dir)
            .arg(&outside_pack)
            .output()
            .expect("run mklink");
        assert!(
            output.status.success(),
            "create runtime pack junction: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let error = resolve_runtime_library(&runtime_root, "stable").unwrap_err();

        assert!(error.contains("escapes trusted runtime root"));
    }

    #[test]
    fn maps_runtime_info_to_dto() {
        let info = RuntimeInfo {
            abi_major: 1,
            abi_minor: 2,
            runtime_version: "0.1.0-fake".into(),
            llama_cpp_commit: "not-linked".into(),
            capabilities: Capabilities {
                supports_cpu: true,
                supports_cuda: false,
                supports_vulkan: false,
                supports_streaming: true,
                supports_cancellation: true,
                max_parallel_slots: 4,
            },
        };

        let dto = runtime_info_dto(&info);

        assert_eq!(dto.abi_major, 1);
        assert_eq!(dto.abi_minor, 2);
        assert_eq!(dto.runtime_version, "0.1.0-fake");
        assert_eq!(dto.llama_cpp_commit, "not-linked");
        assert_eq!(dto.max_parallel_slots, 4);
    }

    #[test]
    fn runtime_info_serializes_with_camel_case_fields() {
        let dto = RuntimeInfoDto {
            abi_major: 1,
            abi_minor: 0,
            runtime_version: "0.1.0-fake".into(),
            llama_cpp_commit: "not-linked".into(),
            max_parallel_slots: 4,
        };
        let value = serde_json::to_value(dto).expect("serialize runtime info");
        assert_eq!(value["runtimeVersion"], "0.1.0-fake");
        assert_eq!(value["maxParallelSlots"], 4);
    }
}
