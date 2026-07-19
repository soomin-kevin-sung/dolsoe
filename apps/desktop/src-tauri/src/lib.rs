mod conversation_commands;
mod conversation_store;
mod llm_commands;
mod llm_dto;
mod llm_worker;
mod runtime_packs;
mod runtime_path;
mod runtime_probe;

use tauri::{Emitter, Manager};

use crate::conversation_store::ConversationStore;
use crate::llm_worker::WorkerHandle;
use crate::runtime_path::RuntimePackResolver;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_data = app.path().app_local_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            let runtime_root = app_data.join("runtime-packs");
            std::fs::create_dir_all(&runtime_root)?;
            let conversation_store = ConversationStore::open(app_data.join("local-llm-wiki.db"))
                .map_err(std::io::Error::other)?;
            let app_handle = app.handle().clone();
            let resolver = RuntimePackResolver::trusted(&app_data, runtime_root)
                .map_err(std::io::Error::other)?;
            let worker = WorkerHandle::spawn(resolver, move |event| {
                app_handle
                    .emit("llm://event", event)
                    .map_err(|error| error.to_string())
            })
            .map_err(std::io::Error::other)?;
            app.manage(conversation_store);
            app.manage(worker);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime_probe::probe_runtime,
            runtime_packs::list_runtime_packs,
            llm_commands::llm_get_status,
            llm_commands::llm_load_model,
            llm_commands::llm_unload_model,
            llm_commands::llm_submit,
            llm_commands::llm_cancel,
            llm_commands::llm_get_metrics,
            conversation_commands::conversation_bootstrap,
            conversation_commands::conversation_create,
            conversation_commands::conversation_load,
            conversation_commands::conversation_rename,
            conversation_commands::conversation_clear,
            conversation_commands::conversation_delete,
            conversation_commands::conversation_start_turn,
            conversation_commands::conversation_finish_turn,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
