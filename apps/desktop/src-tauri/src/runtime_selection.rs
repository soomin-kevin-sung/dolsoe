use std::{fs, io::Write, path::PathBuf, sync::Mutex};

use serde::{Deserialize, Serialize};

use crate::runtime_packs::RuntimeBackend;
use tauri::{AppHandle, State};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSelectionState {
    pub schema_version: u32,
    pub active_backend: RuntimeBackend,
    pub pending_activation: Option<RuntimeBackend>,
    pub last_activation_error: Option<String>,
}

impl Default for RuntimeSelectionState {
    fn default() -> Self {
        Self {
            schema_version: 1,
            active_backend: RuntimeBackend::Cpu,
            pending_activation: None,
            last_activation_error: None,
        }
    }
}

pub struct RuntimeSelectionStore {
    path: PathBuf,
    state: Mutex<RuntimeSelectionState>,
}

impl RuntimeSelectionStore {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let state = match fs::read(&path) {
            Ok(bytes) => {
                let state: RuntimeSelectionState =
                    serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
                if state.schema_version != 1 {
                    return Err("unsupported runtime selection schema".into());
                }
                state
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                RuntimeSelectionState::default()
            }
            Err(error) => return Err(error.to_string()),
        };
        Ok(Self {
            path,
            state: Mutex::new(state),
        })
    }

    pub fn snapshot(&self) -> Result<RuntimeSelectionState, String> {
        self.state
            .lock()
            .map(|state| state.clone())
            .map_err(|_| "runtime selection lock poisoned".into())
    }

    pub fn request_activation(&self, backend: RuntimeBackend) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "runtime selection lock poisoned")?;
        state.pending_activation = Some(backend);
        state.last_activation_error = None;
        persist(&self.path, &state)
    }

    pub fn set_active(&self, backend: RuntimeBackend) -> Result<(), String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "runtime selection lock poisoned")?;
        state.active_backend = backend;
        state.pending_activation = None;
        state.last_activation_error = None;
        persist(&self.path, &state)
    }

    pub fn consume_pending<F>(&self, mut activate: F) -> Result<(), String>
    where
        F: FnMut(RuntimeBackend) -> bool,
    {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "runtime selection lock poisoned")?;
        let Some(pending) = state.pending_activation.take() else {
            return Ok(());
        };
        if activate(pending) {
            state.active_backend = pending;
            state.last_activation_error = None;
        } else {
            state.active_backend = RuntimeBackend::Cpu;
            state.last_activation_error =
                Some(format!("{} activation failed; using CPU", pending.as_str()));
        }
        persist(&self.path, &state)
    }
}

impl RuntimeBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
        }
    }
}

fn persist(path: &std::path::Path, state: &RuntimeSelectionState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    let mut file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
    file.write_all(&serde_json::to_vec(state).map_err(|error| error.to_string())?)
        .map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    drop(file);
    fs::rename(temporary, path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_runtime_selection(
    state: State<'_, RuntimeSelectionStore>,
) -> Result<RuntimeSelectionState, String> {
    state.snapshot()
}

#[tauri::command]
pub fn request_runtime_activation(
    state: State<'_, RuntimeSelectionStore>,
    backend: RuntimeBackend,
) -> Result<(), String> {
    state.request_activation(backend)
}

#[tauri::command]
pub fn set_active_runtime_backend(
    state: State<'_, RuntimeSelectionStore>,
    backend: RuntimeBackend,
) -> Result<(), String> {
    state.set_active(backend)
}

#[tauri::command]
pub fn restart_runtime_app(app: AppHandle) {
    app.restart();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn pending_activation_is_consumed_once_and_persisted() {
        let root = TempDir::new().unwrap();
        let store = RuntimeSelectionStore::open(root.path().join("selection.json")).unwrap();
        store.request_activation(RuntimeBackend::Cuda).unwrap();
        assert_eq!(
            store.snapshot().unwrap().pending_activation,
            Some(RuntimeBackend::Cuda)
        );

        store
            .consume_pending(|backend| backend == RuntimeBackend::Cuda)
            .unwrap();
        let reopened = RuntimeSelectionStore::open(root.path().join("selection.json")).unwrap();
        let state = reopened.snapshot().unwrap();
        assert_eq!(state.active_backend, RuntimeBackend::Cuda);
        assert_eq!(state.pending_activation, None);
    }

    #[test]
    fn failed_pending_activation_falls_back_to_cpu_without_retry_loop() {
        let root = TempDir::new().unwrap();
        let path = root.path().join("selection.json");
        let store = RuntimeSelectionStore::open(path.clone()).unwrap();
        store.request_activation(RuntimeBackend::Vulkan).unwrap();
        let mut attempts = 0;
        store
            .consume_pending(|_| {
                attempts += 1;
                false
            })
            .unwrap();
        store
            .consume_pending(|_| {
                attempts += 1;
                false
            })
            .unwrap();
        let state = RuntimeSelectionStore::open(path)
            .unwrap()
            .snapshot()
            .unwrap();

        assert_eq!(attempts, 1);
        assert_eq!(state.active_backend, RuntimeBackend::Cpu);
        assert_eq!(state.pending_activation, None);
        assert!(state.last_activation_error.is_some());
    }
}
