// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(status) = desktop_lib::run_runtime_probe_cli_if_requested() {
        std::process::exit(status);
    }
    desktop_lib::run()
}
