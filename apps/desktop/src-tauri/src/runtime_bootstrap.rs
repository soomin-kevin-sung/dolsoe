use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{
    runtime_archive::install_verified_archive_with_mode_and_probe,
    runtime_manifest::RuntimeManifestPack,
    runtime_packs::{probe_runtime_directory, runtime_directory_ready},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapState {
    Ready,
    RecoveryRequired(String),
}

pub fn bootstrap_cpu(runtime_root: &Path, resource_root: &Path) -> BootstrapState {
    let installed_cpu = runtime_root.join("cpu");
    let archive = resource_root.join("cpu.zip");
    let index = resource_root.join("cpu-index.json");
    if !archive.is_file() && !index.is_file() {
        return if runtime_directory_ready(&installed_cpu, "cpu") {
            BootstrapState::Ready
        } else {
            BootstrapState::RecoveryRequired(
                "CPU runtime is unavailable and the bundled recovery pack is missing; install CPU from Settings and restart".into(),
            )
        };
    }
    let result = (|| -> Result<(), String> {
        if !archive.is_file() || !index.is_file() {
            return Err("bundled CPU archive or index is missing".into());
        }
        let pack: RuntimeManifestPack =
            serde_json::from_slice(&std::fs::read(&index).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())?;
        if pack.id != "cpu" || pack.backend != "cpu" {
            return Err("bundled CPU index has the wrong identity".into());
        }
        let actual = format!(
            "{:x}",
            Sha256::digest(std::fs::read(&archive).map_err(|error| error.to_string())?)
        );
        if actual != pack.sha256 {
            return Err("bundled CPU archive SHA-256 mismatch".into());
        }
        install_verified_archive_with_mode_and_probe(
            &archive,
            runtime_root,
            &pack,
            false,
            probe_runtime_directory,
        )
        .map_err(|error| error.to_string())?;
        if !runtime_directory_ready(&installed_cpu, "cpu") {
            return Err("installed CPU runtime failed validation or device probe".into());
        }
        Ok(())
    })();
    match result {
        Ok(()) => BootstrapState::Ready,
        Err(_error) if runtime_directory_ready(&installed_cpu, "cpu") => BootstrapState::Ready,
        Err(error) => BootstrapState::RecoveryRequired(format!(
            "CPU runtime recovery failed: {error}; reinstall CPU from Settings and restart"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn missing_bundle_and_cpu_returns_recovery_required_without_failing_startup() {
        let root = TempDir::new().unwrap();
        let resources = TempDir::new().unwrap();
        let state = bootstrap_cpu(root.path(), resources.path());
        assert!(matches!(
            state,
            BootstrapState::RecoveryRequired(error)
                if error.contains("install CPU from Settings and restart")
        ));
    }
}
