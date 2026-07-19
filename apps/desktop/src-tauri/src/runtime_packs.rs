use std::path::Path;

use llm_runtime::{Backend, RuntimeLibrary};
use serde::{Deserialize, Serialize};
use tauri::Manager;

use crate::runtime_path::RuntimePackResolver;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeBackend {
    Cpu,
    Cuda,
    Vulkan,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimePackStatus {
    Ready,
    NotInstalled,
    ReplacementPending,
    RepairRequired,
    Unavailable,
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

fn status_pack(
    id: &str,
    backend: RuntimeBackend,
    status: RuntimePackStatus,
    error: Option<String>,
) -> RuntimePackDto {
    RuntimePackDto {
        id: id.into(),
        backend: Some(backend),
        status,
        runtime_version: None,
        llama_cpp_commit: None,
        abi_major: None,
        abi_minor: None,
        devices: Vec::new(),
        error,
    }
}

fn scan_runtime_packs<F>(root: &Path, mut probe: F) -> Result<RuntimePackInventoryDto, String>
where
    F: FnMut(&str, &Path) -> Result<ProbedPack, String>,
{
    let mut packs = Vec::new();
    for (id, expected_backend) in [
        ("cpu", RuntimeBackend::Cpu),
        ("cuda", RuntimeBackend::Cuda),
        ("vulkan", RuntimeBackend::Vulkan),
    ] {
        let path = root.join(id);
        let replacement_pending = root
            .join(".transactions")
            .join(format!("{id}.json"))
            .is_file();
        if !path.is_dir() {
            packs.push(status_pack(
                id,
                expected_backend,
                RuntimePackStatus::NotInstalled,
                None,
            ));
            continue;
        }
        let pack = match probe(id, &path) {
            Ok(value) => {
                let unavailable = value.devices.is_empty();
                RuntimePackDto {
                    id: id.into(),
                    backend: Some(value.backend),
                    status: if replacement_pending {
                        RuntimePackStatus::ReplacementPending
                    } else if unavailable {
                        RuntimePackStatus::Unavailable
                    } else {
                        RuntimePackStatus::Ready
                    },
                    runtime_version: Some(value.runtime_version),
                    llama_cpp_commit: Some(value.llama_cpp_commit),
                    abi_major: Some(value.abi_major),
                    abi_minor: Some(value.abi_minor),
                    devices: value.devices,
                    error: unavailable
                        .then(|| "runtime pack has no available device or driver".into()),
                }
            }
            Err(error) => status_pack(
                id,
                expected_backend,
                RuntimePackStatus::RepairRequired,
                Some(error),
            ),
        };
        packs.push(pack);
    }
    let fallback_pack_id = select_fallback(&packs).map(|pack| pack.id.clone());
    Ok(RuntimePackInventoryDto {
        packs,
        fallback_pack_id,
    })
}

fn select_fallback(packs: &[RuntimePackDto]) -> Option<&RuntimePackDto> {
    packs
        .iter()
        .find(|pack| pack.id == "cpu" && pack.status == RuntimePackStatus::Ready)
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
    let expected = match id {
        "cpu" => RuntimeBackend::Cpu,
        "cuda" => RuntimeBackend::Cuda,
        "vulkan" => RuntimeBackend::Vulkan,
        _ => return Err("unsupported runtime backend ID".into()),
    };
    if backend != expected {
        return Err("runtime pack capability does not match its backend ID".into());
    }
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
    let app_data = app
        .path()
        .app_local_data_dir()
        .map_err(|error| error.to_string())?;
    let runtime_root = app_data.join("runtime-packs");
    tauri::async_runtime::spawn_blocking(move || {
        let resolver = RuntimePackResolver::trusted(&app_data, runtime_root.clone())?;
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
        create_pack(&root, "cuda");
        create_pack(&root, "cpu");

        let inventory = scan_runtime_packs(root.path(), |id, _| match id {
            "cpu" => Ok(probed_cpu("CPU 0")),
            _ => Err("ABI mismatch".into()),
        })
        .expect("scan runtime packs");

        assert_eq!(inventory.packs.len(), 3);
        assert_eq!(inventory.packs[0].id, "cpu");
        assert_eq!(inventory.packs[0].status, RuntimePackStatus::Ready);
        assert_eq!(inventory.packs[1].id, "cuda");
        assert_eq!(inventory.packs[1].status, RuntimePackStatus::RepairRequired);
        assert_eq!(inventory.packs[1].error.as_deref(), Some("ABI mismatch"));
        assert_eq!(inventory.packs[2].status, RuntimePackStatus::NotInstalled);
        assert_eq!(inventory.fallback_pack_id.as_deref(), Some("cpu"));
    }

    #[test]
    fn fallback_uses_only_ready_stable_cpu() {
        let packs = vec![
            ready("vulkan-a", RuntimeBackend::Vulkan),
            ready("cpu", RuntimeBackend::Cpu),
        ];
        assert_eq!(
            select_fallback(&packs).map(|pack| pack.id.as_str()),
            Some("cpu")
        );

        let packs = vec![ready("vulkan-a", RuntimeBackend::Vulkan)];
        assert_eq!(select_fallback(&packs), None);
    }

    #[test]
    fn ready_pack_requires_at_least_one_device() {
        let root = TempDir::new().expect("create trusted runtime root");
        create_pack(&root, "cpu");
        let mut probe = probed_cpu("unused");
        probe.devices.clear();

        let inventory =
            scan_runtime_packs(root.path(), |_, _| Ok(probe.clone())).expect("scan runtime packs");

        assert_eq!(inventory.packs[0].status, RuntimePackStatus::Unavailable);
        assert!(inventory.packs[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("device")));
        assert_eq!(inventory.fallback_pack_id, None);
    }

    #[test]
    fn production_inventory_uses_only_stable_backend_ids_and_cpu_fallback() {
        let root = TempDir::new().expect("create trusted runtime root");
        create_pack(&root, "cpu");
        create_pack(&root, "cpu-dev");
        create_pack(&root, "rogue");

        let inventory = scan_runtime_packs(root.path(), |id, _| {
            if id == "cpu" {
                Ok(probed_cpu("CPU 0"))
            } else {
                Err("unexpected".into())
            }
        })
        .unwrap();

        assert_eq!(
            inventory
                .packs
                .iter()
                .map(|pack| pack.id.as_str())
                .collect::<Vec<_>>(),
            vec!["cpu", "cuda", "vulkan"]
        );
        assert_eq!(inventory.fallback_pack_id.as_deref(), Some("cpu"));
    }
}
