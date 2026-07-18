mod llm_commands;
mod llm_dto;
mod llm_worker;
mod runtime_path;
mod runtime_probe;

use tauri::{Emitter, Manager};

use crate::llm_worker::WorkerHandle;
use crate::runtime_path::RuntimePackResolver;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let runtime_root = app.path().app_local_data_dir()?.join("runtime-packs");
            let app_handle = app.handle().clone();
            let worker =
                WorkerHandle::spawn(RuntimePackResolver::new(runtime_root), move |event| {
                    app_handle
                        .emit("llm://event", event)
                        .map_err(|error| error.to_string())
                })
                .map_err(std::io::Error::other)?;
            app.manage(worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime_probe::probe_runtime,
            llm_commands::llm_get_status,
            llm_commands::llm_load_model,
            llm_commands::llm_unload_model,
            llm_commands::llm_submit,
            llm_commands::llm_cancel,
            llm_commands::llm_get_metrics,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
