use std::{
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use llm_runtime::{Backend, RuntimeLibrary};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::{
    runtime_archive::validate_installed_pack, runtime_manifest::bundled_manifest_policy,
    runtime_path::RuntimePackResolver,
};

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

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
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
        if root.join(format!(".repair-required-{id}")).is_file() {
            packs.push(status_pack(
                id,
                expected_backend,
                RuntimePackStatus::RepairRequired,
                Some("runtime recovery failed; reinstall this backend".into()),
            ));
            continue;
        }
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
    let directory = resolver.runtime_root().join(id);
    if !validate_installed_pack(&directory, id) {
        return Err("runtime pack file validation failed".into());
    }
    let path = resolver.resolve(id)?;
    probe_runtime_path(&path, id)
}

fn probe_runtime_path(path: &Path, id: &str) -> Result<ProbedPack, String> {
    let mut command = Command::new(std::env::current_exe().map_err(|error| error.to_string())?);
    command.args(["--runtime-probe", id]).arg(path);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|error| format!("failed to start runtime probe: {error}"))?;
    let output = wait_for_probe_process(child, Duration::from_secs(10))?;
    if !output.status.success() {
        return Err(format!(
            "runtime probe failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let probe: ProbedPack = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid runtime probe response: {error}"))?;
    validate_probed_pack(id, &probe)?;
    Ok(probe)
}

pub(crate) fn probe_runtime_directory(directory: &Path, id: &str) -> Result<(), String> {
    expected_backend(id)?;
    let canonical_directory = directory
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize runtime directory: {error}"))?;
    let library = directory.join(crate::runtime_path::runtime_library_filename());
    let canonical_library = library
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize runtime library: {error}"))?;
    if canonical_library.parent() != Some(canonical_directory.as_path()) {
        return Err("runtime library leaves its pack directory".into());
    }
    probe_runtime_path(&canonical_library, id).map(|_| ())
}

pub(crate) fn runtime_directory_ready(directory: &Path, id: &str) -> bool {
    if !validate_installed_pack(directory, id) {
        return false;
    }
    let library = directory.join(crate::runtime_path::runtime_library_filename());
    probe_runtime_path(&library, id).is_ok_and(|probe| !probe.devices.is_empty())
}

fn wait_for_probe_process(mut child: Child, timeout: Duration) -> Result<Output, String> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map_err(|error| format!("failed to collect runtime probe output: {error}"));
            }
            Ok(None) if started.elapsed() < timeout => {
                thread::sleep(Duration::from_millis(25));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "runtime probe timed out after {} seconds",
                    timeout.as_secs_f32()
                ));
            }
            Err(error) => return Err(format!("failed to wait for runtime probe: {error}")),
        }
    }
}

fn probe_runtime_library(path: &Path, id: &str) -> Result<ProbedPack, String> {
    // The managed runtime root is exclusively populated by the runtime pack installer.
    let runtime = unsafe { RuntimeLibrary::load(path) }.map_err(|error| error.to_string())?;
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
    let expected = expected_backend(id)?;
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

fn expected_backend(id: &str) -> Result<RuntimeBackend, String> {
    Ok(match id {
        "cpu" => RuntimeBackend::Cpu,
        "cuda" => RuntimeBackend::Cuda,
        "vulkan" => RuntimeBackend::Vulkan,
        _ => return Err("unsupported runtime backend ID".into()),
    })
}

fn validate_probed_pack(id: &str, probe: &ProbedPack) -> Result<(), String> {
    let policy = bundled_manifest_policy()?;
    if probe.backend != expected_backend(id)?
        || probe.llama_cpp_commit != policy.llama_cpp_commit
        || probe.abi_major != policy.abi_major
        || probe.abi_minor != policy.abi_minor
    {
        return Err("runtime probe identity does not match the bundled baseline".into());
    }
    Ok(())
}

pub(crate) fn backend_ready(resolver: &RuntimePackResolver, backend: RuntimeBackend) -> bool {
    let id = backend.as_str();
    let directory = resolver.runtime_root().join(id);
    runtime_directory_ready(&directory, id)
}

pub(crate) fn run_runtime_probe_cli(args: &[String]) -> Option<Result<String, String>> {
    if args.len() != 4 || args[1] != "--runtime-probe" {
        return None;
    }
    Some(
        probe_runtime_library(Path::new(&args[3]), &args[2])
            .and_then(|probe| validate_probed_pack(&args[2], &probe).map(|_| probe))
            .and_then(|probe| serde_json::to_string(&probe).map_err(|error| error.to_string())),
    )
}

#[tauri::command]
pub async fn list_runtime_packs(
    resolver: State<'_, RuntimePackResolver>,
) -> Result<RuntimePackInventoryDto, String> {
    let resolver = resolver.inner().clone();
    let runtime_root = resolver.runtime_root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        scan_runtime_packs(&runtime_root, |id, _| probe_runtime_pack(&resolver, id))
    })
    .await
    .map_err(|error| format!("runtime pack inventory task failed: {error}"))?
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command, time::Duration};

    use tempfile::TempDir;

    use super::{
        scan_runtime_packs, select_fallback, validate_probed_pack, wait_for_probe_process,
        ProbedPack, RuntimeBackend, RuntimeDeviceDto, RuntimePackDto, RuntimePackStatus,
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

    #[test]
    fn isolated_transaction_failure_requires_backend_repair() {
        let root = TempDir::new().expect("create trusted runtime root");
        fs::write(root.path().join(".repair-required-cuda"), b"").expect("write repair marker");

        let inventory = scan_runtime_packs(root.path(), |_, _| {
            panic!("repair marker must prevent probing")
        })
        .unwrap();

        assert_eq!(inventory.packs[1].status, RuntimePackStatus::RepairRequired);
        assert!(inventory.packs[1]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("recovery")));
    }

    #[test]
    fn probed_runtime_identity_must_match_current_baseline() {
        let mut probe = probed_cpu("CPU 0");
        probe.llama_cpp_commit = "571d0d540df04f25298d0e159e520d9fc62ed121".into();
        probe.abi_minor = 3;
        assert!(validate_probed_pack("cpu", &probe).is_ok());

        probe.llama_cpp_commit = "stale-commit".into();
        assert!(validate_probed_pack("cpu", &probe)
            .unwrap_err()
            .contains("baseline"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn runtime_probe_process_is_killed_after_timeout() {
        let child = Command::new("powershell")
            .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 5"])
            .spawn()
            .unwrap();

        let error = wait_for_probe_process(child, Duration::from_millis(10)).unwrap_err();

        assert!(error.contains("timed out"));
    }
}
