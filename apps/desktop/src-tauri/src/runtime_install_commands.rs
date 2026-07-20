use std::{collections::HashSet, fs, path::PathBuf, sync::Arc};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    runtime_installer::{RuntimeDistributionConfig, RuntimeInstaller},
    runtime_manifest::RuntimeManifestPack,
    runtime_packs::{backend_ready, RuntimeBackend},
    runtime_path::RuntimePackResolver,
    runtime_selection::RuntimeSelectionStore,
};

pub struct RuntimeInstallerState {
    installer: Option<Arc<RuntimeInstaller>>,
    runtime_root: PathBuf,
    configuration_error: Option<String>,
}

impl RuntimeInstallerState {
    pub fn from_app_data(app_data: &std::path::Path, runtime_root: PathBuf) -> Self {
        match RuntimeDistributionConfig::from_app_data(app_data) {
            Ok(config) => Self {
                installer: Some(Arc::new(RuntimeInstaller::new(
                    runtime_root.clone(),
                    config,
                ))),
                runtime_root,
                configuration_error: None,
            },
            Err(error) => Self {
                installer: None,
                runtime_root,
                configuration_error: Some(error),
            },
        }
    }

    fn installer(&self) -> Result<Arc<RuntimeInstaller>, String> {
        self.installer.clone().ok_or_else(|| {
            self.configuration_error
                .clone()
                .unwrap_or_else(|| "runtime distribution is unavailable".into())
        })
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AvailableRuntimePackDto {
    pub id: String,
    pub backend: String,
    pub release_version: String,
    pub size_bytes: u64,
    pub llama_cpp_release: String,
    pub llama_cpp_commit: String,
    pub installed: bool,
}

pub fn available_pack_dtos(
    packs: Vec<RuntimeManifestPack>,
    installed: &HashSet<String>,
    release_version: &str,
) -> Vec<AvailableRuntimePackDto> {
    let mut values: Vec<_> = packs
        .into_iter()
        .map(|pack| AvailableRuntimePackDto {
            installed: installed.contains(&pack.id),
            id: pack.id,
            backend: pack.backend,
            release_version: release_version.into(),
            size_bytes: pack.size,
            llama_cpp_release: pack.llama_cpp_release,
            llama_cpp_commit: pack.llama_cpp_commit,
        })
        .collect();
    values.sort_by(|left, right| left.id.cmp(&right.id));
    values
}

#[tauri::command]
pub async fn list_available_runtime_packs(
    state: State<'_, RuntimeInstallerState>,
) -> Result<Vec<AvailableRuntimePackDto>, String> {
    let installer = state.installer()?;
    let release = installer
        .available_packs()
        .await
        .map_err(|error| error.to_string())?;
    let installed = installed_pack_ids(&state.runtime_root)?;
    Ok(available_pack_dtos(
        release.packs,
        &installed,
        &release.release_version,
    ))
}

#[tauri::command]
pub async fn install_runtime_pack(
    app: AppHandle,
    state: State<'_, RuntimeInstallerState>,
    selection: State<'_, RuntimeSelectionStore>,
    pack_id: String,
) -> Result<(), String> {
    let backend = match pack_id.as_str() {
        "cuda" => RuntimeBackend::Cuda,
        "vulkan" => RuntimeBackend::Vulkan,
        _ => return Err("only CUDA and Vulkan runtime packs can be downloaded".into()),
    };
    let defer_replacement = selection.snapshot()?.active_backend == backend;
    let installer = state.installer()?;
    let progress_app = app.clone();
    installer
        .install(&pack_id, defer_replacement, move |progress| {
            let _ = progress_app.emit("runtime-pack-install-progress", progress);
        })
        .await
        .map_err(|error| error.to_string())?;
    let app_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let resolver = RuntimePackResolver::trusted(&app_data, state.runtime_root.clone())?;
    if backend_ready(&resolver, backend) {
        selection.request_activation(backend)?;
    }
    Ok(())
}

#[tauri::command]
pub fn cancel_runtime_pack_install(state: State<'_, RuntimeInstallerState>) -> Result<(), String> {
    state.installer()?.cancel()
}

fn installed_pack_ids(runtime_root: &PathBuf) -> Result<HashSet<String>, String> {
    let entries = fs::read_dir(runtime_root)
        .map_err(|error| format!("failed to read installed runtime packs: {error}"))?;
    Ok(entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            (!id.starts_with('.')).then_some(id)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::runtime_manifest::RuntimeManifestPack;

    use super::available_pack_dtos;

    fn pack(id: &str, backend: &str) -> RuntimeManifestPack {
        RuntimeManifestPack {
            id: id.into(),
            backend: backend.into(),
            pack_version: "2026.07.1".into(),
            platform: "windows".into(),
            arch: "x86_64".into(),
            llama_cpp_release: "b10068".into(),
            llama_cpp_commit: "571d0d540df04f25298d0e159e520d9fc62ed121".into(),
            abi_major: 1,
            abi_minor: 2,
            asset_name: "pack.zip".into(),
            size: 1024,
            sha256: "0".repeat(64),
        }
    }

    #[test]
    fn maps_available_packs_and_installed_state_to_camel_case_dtos() {
        let installed = HashSet::from(["cpu".to_owned()]);
        let dtos = available_pack_dtos(
            vec![pack("cuda", "cuda"), pack("cpu", "cpu")],
            &installed,
            "2026.07.1",
        );

        assert_eq!(dtos[0].id, "cpu");
        assert!(dtos[0].installed);
        assert_eq!(dtos[1].backend, "cuda");
        assert!(!dtos[1].installed);
        let value = serde_json::to_value(&dtos[1]).unwrap();
        assert_eq!(value["releaseVersion"], "2026.07.1");
        assert_eq!(value["sizeBytes"], 1024);
    }
}
