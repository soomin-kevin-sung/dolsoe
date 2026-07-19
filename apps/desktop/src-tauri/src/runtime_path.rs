use std::path::{Path, PathBuf};

const MAX_RUNTIME_PACK_ID_LEN: usize = 64;

#[derive(Debug, Clone)]
pub struct RuntimePackResolver {
    runtime_root: PathBuf,
}

impl RuntimePackResolver {
    pub fn new(runtime_root: PathBuf) -> Self {
        Self { runtime_root }
    }

    pub fn trusted(app_local_data: &Path, runtime_root: PathBuf) -> Result<Self, String> {
        let canonical_app_data = app_local_data
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize app-local data: {error}"))?;
        let canonical_runtime_root = runtime_root
            .canonicalize()
            .map_err(|error| format!("failed to canonicalize trusted runtime root: {error}"))?;
        if canonical_runtime_root.parent() != Some(canonical_app_data.as_path()) {
            return Err("trusted runtime root must be a direct child of app-local data".into());
        }
        Ok(Self::new(canonical_runtime_root))
    }

    pub fn runtime_root(&self) -> &Path {
        &self.runtime_root
    }

    pub fn resolve(&self, runtime_pack_id: &str) -> Result<PathBuf, String> {
        resolve_runtime_library(&self.runtime_root, runtime_pack_id)
    }
}

pub(crate) fn validate_runtime_pack_id(runtime_pack_id: &str) -> Result<(), String> {
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
pub(crate) fn runtime_library_filename() -> &'static str {
    "local_llm_runtime.dll"
}

#[cfg(target_os = "macos")]
pub(crate) fn runtime_library_filename() -> &'static str {
    "liblocal_llm_runtime.dylib"
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn runtime_library_filename() -> &'static str {
    "liblocal_llm_runtime.so"
}

pub(crate) fn resolve_runtime_library(
    runtime_root: &Path,
    runtime_pack_id: &str,
) -> Result<PathBuf, String> {
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

#[cfg(test)]
mod tests {
    use std::fs;

    use super::RuntimePackResolver;

    #[test]
    fn resolves_a_library_beneath_the_trusted_root() {
        let temp = tempfile::tempdir().expect("create temporary directory");
        let root = temp.path().join("runtime-packs");
        let pack = root.join("cpu-dev");
        fs::create_dir_all(&pack).expect("create pack directory");
        let library = pack.join(super::runtime_library_filename());
        fs::write(&library, b"fixture").expect("write runtime fixture");

        let resolved = RuntimePackResolver::new(root)
            .resolve("cpu-dev")
            .expect("resolve managed runtime");

        assert_eq!(resolved, library.canonicalize().unwrap());
    }

    #[test]
    fn rejects_traversal_pack_ids() {
        let temp = tempfile::tempdir().expect("create temporary directory");
        let resolver = RuntimePackResolver::new(temp.path().to_path_buf());

        assert!(resolver.resolve("../outside").is_err());
        assert!(resolver.resolve("pack\\outside").is_err());
    }

    #[test]
    fn accepts_release_runtime_pack_ids() {
        for runtime_pack_id in ["stable", "llama.cpp-1_2.3", "CUDA12", "a"] {
            assert!(super::validate_runtime_pack_id(runtime_pack_id).is_ok());
        }
    }

    #[test]
    fn rejects_invalid_runtime_pack_ids() {
        let overlong = "a".repeat(super::MAX_RUNTIME_PACK_ID_LEN + 1);
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
            assert!(super::validate_runtime_pack_id(runtime_pack_id).is_err());
        }
    }

    #[test]
    fn missing_pack_error_names_the_expected_directory() {
        let temp = tempfile::tempdir().expect("create temporary directory");
        let root = temp.path().join("runtime-packs");
        fs::create_dir(&root).expect("create runtime root");

        let error = RuntimePackResolver::new(root.clone())
            .resolve("cpu-dev")
            .unwrap_err();

        assert!(error.contains(&root.join("cpu-dev").display().to_string()));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn rejects_runtime_root_junction_outside_app_data() {
        use std::process::Command;

        let temp = tempfile::tempdir().expect("create temporary directory");
        let app_data = temp.path().join("app-data");
        let runtime_root = app_data.join("runtime-packs");
        let outside = temp.path().join("outside-runtime-packs");
        fs::create_dir(&app_data).expect("create app data");
        fs::create_dir(&outside).expect("create outside root");
        let output = Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&runtime_root)
            .arg(&outside)
            .output()
            .expect("run mklink");
        assert!(output.status.success(), "create runtime root junction");

        let error = RuntimePackResolver::trusted(&app_data, runtime_root).unwrap_err();

        assert!(error.contains("direct child of app-local data"));
    }
}
