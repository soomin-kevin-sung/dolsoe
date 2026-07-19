use std::path::Path;

use llm_runtime::{Backend, RuntimeLibrary};
use serde::Serialize;
use tauri::Manager;

use crate::runtime_path::{validate_runtime_pack_id, RuntimePackResolver};

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeBackend {
    Cpu,
    Cuda,
    Vulkan,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimePackStatus {
    Ready,
    Invalid,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDeviceDto {
    pub index: u32,
    pub id: String,
    pub name: String,
    pub vendor: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackDto {
    pub id: String,
    pub backend: Option<RuntimeBackend>,
    pub status: RuntimePackStatus,
    pub runtime_version: Option<String>,
    pub llama_cpp_commit: Option<String>,
    pub abi_major: Option<u32>,
    pub abi_minor: Option<u32>,
    pub devices: Vec<RuntimeDeviceDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackInventoryDto {
    pub packs: Vec<RuntimePackDto>,
    pub fallback_pack_id: Option<String>,
}

#[derive(Debug, Clone)]
struct ProbedPack {
    backend: RuntimeBackend,
    runtime_version: String,
    llama_cpp_commit: String,
    abi_major: u32,
    abi_minor: u32,
    devices: Vec<RuntimeDeviceDto>,
}

fn invalid_pack(id: String, error: String) -> RuntimePackDto {
    RuntimePackDto {
        id,
        backend: None,
        status: RuntimePackStatus::Invalid,
        runtime_version: None,
        llama_cpp_commit: None,
        abi_major: None,
        abi_minor: None,
        devices: Vec::new(),
        error: Some(error),
    }
}

fn scan_runtime_packs<F>(root: &Path, mut probe: F) -> Result<RuntimePackInventoryDto, String>
where
    F: FnMut(&str, &Path) -> Result<ProbedPack, String>,
{
    let entries = std::fs::read_dir(root).map_err(|error| {
        format!(
            "failed to read runtime pack root {}: {error}",
            root.display()
        )
    })?;
    let mut packs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("failed to read runtime pack entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("failed to inspect runtime pack entry: {error}"))?;
        if !file_type.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        let result = validate_runtime_pack_id(&id)
            .and_then(|_| probe(&id, &entry.path()))
            .and_then(|value| {
                if value.devices.is_empty() {
                    Err("runtime pack has no device for its selected backend".into())
                } else {
                    Ok(value)
                }
            });
        let pack = match result {
            Ok(value) => RuntimePackDto {
                id,
                backend: Some(value.backend),
                status: RuntimePackStatus::Ready,
                runtime_version: Some(value.runtime_version),
                llama_cpp_commit: Some(value.llama_cpp_commit),
                abi_major: Some(value.abi_major),
                abi_minor: Some(value.abi_minor),
                devices: value.devices,
                error: None,
            },
            Err(error) => invalid_pack(id, error),
        };
        packs.push(pack);
    }
    packs.sort_by(|left, right| left.id.cmp(&right.id));
    let fallback_pack_id = select_fallback(&packs).map(|pack| pack.id.clone());
    Ok(RuntimePackInventoryDto {
        packs,
        fallback_pack_id,
    })
}

fn select_fallback(packs: &[RuntimePackDto]) -> Option<&RuntimePackDto> {
    let ready = |pack: &&RuntimePackDto| pack.status == RuntimePackStatus::Ready;
    packs
        .iter()
        .find(|pack| pack.id == "cpu-dev" && pack.backend == Some(RuntimeBackend::Cpu))
        .filter(ready)
        .or_else(|| {
            packs
                .iter()
                .filter(ready)
                .find(|pack| pack.backend == Some(RuntimeBackend::Cpu))
        })
        .or_else(|| packs.iter().find(ready))
}

fn probe_runtime_pack(resolver: &RuntimePackResolver, id: &str) -> Result<ProbedPack, String> {
    let path = resolver.resolve(id)?;
    // The managed runtime root is exclusively populated by the runtime pack installer.
    let runtime = unsafe { RuntimeLibrary::load(&path) }.map_err(|error| error.to_string())?;
    let info = runtime.info();
    let (backend, native_backend) = if info.capabilities.supports_cuda {
        (RuntimeBackend::Cuda, Backend::Cuda)
    } else if info.capabilities.supports_vulkan {
        (RuntimeBackend::Vulkan, Backend::Vulkan)
    } else if info.capabilities.supports_cpu {
        (RuntimeBackend::Cpu, Backend::Cpu)
    } else {
        return Err("runtime pack reports no supported backend".into());
    };
    let devices = runtime
        .devices(native_backend)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|device| RuntimeDeviceDto {
            index: device.device_index,
            id: device.id,
            name: device.name,
            vendor: device.vendor,
        })
        .collect();
    Ok(ProbedPack {
        backend,
        runtime_version: info.runtime_version.clone(),
        llama_cpp_commit: info.llama_cpp_commit.clone(),
        abi_major: info.abi_major,
        abi_minor: info.abi_minor,
        devices,
    })
}

#[tauri::command]
pub async fn list_runtime_packs(app: tauri::AppHandle) -> Result<RuntimePackInventoryDto, String> {
    let runtime_root = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?
        .join("runtime-packs");
    tauri::async_runtime::spawn_blocking(move || {
        let resolver = RuntimePackResolver::new(runtime_root.clone());
        scan_runtime_packs(&runtime_root, |id, _| probe_runtime_pack(&resolver, id))
    })
    .await
    .map_err(|error| format!("runtime pack inventory task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::{
        scan_runtime_packs, select_fallback, ProbedPack, RuntimeBackend, RuntimeDeviceDto,
        RuntimePackDto, RuntimePackStatus,
    };

    fn create_pack(root: &TempDir, id: &str) {
        fs::create_dir_all(root.path().join(id)).expect("create runtime pack fixture");
    }

    fn probed_cpu(name: &str) -> ProbedPack {
        ProbedPack {
            backend: RuntimeBackend::Cpu,
            runtime_version: "0.1.0".into(),
            llama_cpp_commit: "test-commit".into(),
            abi_major: 1,
            abi_minor: 0,
            devices: vec![RuntimeDeviceDto {
                index: 0,
                id: "cpu:0".into(),
                name: name.into(),
                vendor: "Test".into(),
            }],
        }
    }

    fn ready(id: &str, backend: RuntimeBackend) -> RuntimePackDto {
        RuntimePackDto {
            id: id.into(),
            backend: Some(backend),
            status: RuntimePackStatus::Ready,
            runtime_version: Some("0.1.0".into()),
            llama_cpp_commit: Some("test-commit".into()),
            abi_major: Some(1),
            abi_minor: Some(0),
            devices: vec![RuntimeDeviceDto {
                index: 0,
                id: format!("{id}:0"),
                name: id.into(),
                vendor: "Test".into(),
            }],
            error: None,
        }
    }

    #[test]
    fn inventory_keeps_invalid_packs_without_failing_ready_packs() {
        let root = TempDir::new().expect("create trusted runtime root");
        create_pack(&root, "broken");
        create_pack(&root, "cpu-dev");

        let inventory = scan_runtime_packs(root.path(), |id, _| match id {
            "cpu-dev" => Ok(probed_cpu("CPU 0")),
            _ => Err("ABI mismatch".into()),
        })
        .expect("scan runtime packs");

        assert_eq!(inventory.packs.len(), 2);
        assert_eq!(inventory.packs[0].id, "broken");
        assert_eq!(inventory.packs[0].status, RuntimePackStatus::Invalid);
        assert_eq!(inventory.packs[0].error.as_deref(), Some("ABI mismatch"));
        assert_eq!(inventory.packs[1].id, "cpu-dev");
        assert_eq!(inventory.packs[1].backend, Some(RuntimeBackend::Cpu));
        assert_eq!(inventory.fallback_pack_id.as_deref(), Some("cpu-dev"));
    }

    #[test]
    fn fallback_prefers_cpu_dev_then_other_cpu_then_any_ready_pack() {
        let packs = vec![
            ready("vulkan-a", RuntimeBackend::Vulkan),
            ready("cpu-z", RuntimeBackend::Cpu),
            ready("cpu-dev", RuntimeBackend::Cpu),
        ];
        assert_eq!(
            select_fallback(&packs).map(|pack| pack.id.as_str()),
            Some("cpu-dev")
        );

        let packs = vec![
            ready("vulkan-a", RuntimeBackend::Vulkan),
            ready("cpu-z", RuntimeBackend::Cpu),
        ];
        assert_eq!(
            select_fallback(&packs).map(|pack| pack.id.as_str()),
            Some("cpu-z")
        );

        let packs = vec![ready("vulkan-a", RuntimeBackend::Vulkan)];
        assert_eq!(
            select_fallback(&packs).map(|pack| pack.id.as_str()),
            Some("vulkan-a")
        );
    }

    #[test]
    fn ready_pack_requires_at_least_one_device() {
        let root = TempDir::new().expect("create trusted runtime root");
        create_pack(&root, "cpu-empty");
        let mut probe = probed_cpu("unused");
        probe.devices.clear();

        let inventory =
            scan_runtime_packs(root.path(), |_, _| Ok(probe.clone())).expect("scan runtime packs");

        assert_eq!(inventory.packs[0].status, RuntimePackStatus::Invalid);
        assert!(inventory.packs[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("device")));
        assert_eq!(inventory.fallback_pack_id, None);
    }
}
