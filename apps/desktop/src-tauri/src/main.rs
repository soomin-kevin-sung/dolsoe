// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(status) = dolsoe_desktop_lib::run_runtime_probe_cli_if_requested() {
        std::process::exit(status);
    }
    dolsoe_desktop_lib::run()
}
