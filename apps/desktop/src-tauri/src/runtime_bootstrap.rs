use std::path::Path;

use sha2::{Digest, Sha256};

use crate::{
    runtime_archive::{install_verified_archive, validate_installed_pack},
    runtime_manifest::RuntimeManifestPack,
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
        return if validate_installed_pack(&installed_cpu, "cpu") {
            BootstrapState::Ready
        } else {
            BootstrapState::RecoveryRequired(
                "CPU runtime is unavailable and the bundled recovery pack is missing".into(),
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
        install_verified_archive(&archive, runtime_root, &pack)
            .map_err(|error| error.to_string())?;
        if !validate_installed_pack(&installed_cpu, "cpu") {
            return Err("installed CPU runtime failed validation".into());
        }
        Ok(())
    })();
    match result {
        Ok(()) => BootstrapState::Ready,
        Err(_error) if validate_installed_pack(&installed_cpu, "cpu") => BootstrapState::Ready,
        Err(error) => {
            BootstrapState::RecoveryRequired(format!("CPU runtime recovery failed: {error}"))
        }
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
        assert!(matches!(state, BootstrapState::RecoveryRequired(_)));
    }
}
