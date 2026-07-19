mod conversation_commands;
mod conversation_store;
mod llm_commands;
mod llm_dto;
mod llm_worker;
mod runtime_archive;
mod runtime_bootstrap;
mod runtime_download;
mod runtime_host;
mod runtime_install_commands;
mod runtime_installer;
mod runtime_manifest;
mod runtime_packs;
mod runtime_path;
mod runtime_probe;
mod runtime_selection;
mod runtime_source;
mod runtime_transaction;

use tauri::{Emitter, Manager};

use crate::conversation_store::ConversationStore;
use crate::llm_worker::WorkerHandle;
use crate::runtime_bootstrap::BootstrapState;
use crate::runtime_host::RuntimeHost;
use crate::runtime_install_commands::RuntimeInstallerState;
use crate::runtime_path::RuntimePackResolver;
use crate::runtime_selection::RuntimeSelectionStore;

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
            runtime_transaction::recover_transactions(
                &runtime_root,
                runtime_archive::validate_installed_pack_self,
            )
            .map_err(std::io::Error::other)?;
            let resource_root = app.path().resource_dir()?.join("runtime-packs");
            let bootstrap = runtime_bootstrap::bootstrap_cpu(&runtime_root, &resource_root);
            let conversation_store = ConversationStore::open(app_data.join("local-llm-wiki.db"))
                .map_err(std::io::Error::other)?;
            let selection_store =
                RuntimeSelectionStore::open(app_data.join("runtime-selection.json"))
                    .map_err(std::io::Error::other)?;
            let resolver = RuntimePackResolver::trusted(&app_data, runtime_root.clone())
                .map_err(std::io::Error::other)?;
            selection_store
                .consume_pending(|backend| runtime_packs::backend_ready(&resolver, backend))
                .map_err(std::io::Error::other)?;
            let app_handle = app.handle().clone();
            let host = match bootstrap {
                BootstrapState::Ready => RuntimeHost::ready(
                    WorkerHandle::spawn(resolver, move |event| {
                        app_handle
                            .emit("llm://event", event)
                            .map_err(|error| error.to_string())
                    })
                    .map_err(std::io::Error::other)?,
                ),
                BootstrapState::RecoveryRequired(error) => RuntimeHost::recovery(error),
            };
            app.manage(conversation_store);
            app.manage(selection_store);
            app.manage(host);
            app.manage(RuntimeInstallerState::from_app_data(
                &app_data,
                runtime_root,
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime_probe::probe_runtime,
            runtime_packs::list_runtime_packs,
            runtime_install_commands::list_available_runtime_packs,
            runtime_install_commands::install_runtime_pack,
            runtime_install_commands::cancel_runtime_pack_install,
            runtime_selection::get_runtime_selection,
            runtime_selection::request_runtime_activation,
            runtime_selection::set_active_runtime_backend,
            runtime_selection::restart_runtime_app,
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

pub fn run_runtime_probe_cli_if_requested() -> Option<i32> {
    runtime_packs::run_runtime_probe_cli(&std::env::args().collect::<Vec<_>>()).map(|result| {
        match result {
            Ok(json) => {
                println!("{json}");
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        }
    })
}
