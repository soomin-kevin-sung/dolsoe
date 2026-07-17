use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfoDto {
    pub abi_major: u32,
    pub abi_minor: u32,
    pub runtime_version: String,
    pub llama_cpp_commit: String,
    pub max_parallel_slots: u32,
}

#[tauri::command]
pub async fn probe_runtime(path: PathBuf) -> Result<RuntimeInfoDto, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // SAFETY: This command is only valid for trusted, project-managed runtime packs
        // that conform to the expected ABI.
        let runtime = unsafe { llm_runtime::RuntimeLibrary::load(&path) }
            .map_err(|error| error.to_string())?;
        let info = runtime.info();
        Ok(RuntimeInfoDto {
            abi_major: info.abi_major,
            abi_minor: info.abi_minor,
            runtime_version: info.runtime_version.clone(),
            llama_cpp_commit: info.llama_cpp_commit.clone(),
            max_parallel_slots: info.capabilities.max_parallel_slots,
        })
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

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
