#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(commands::app_state())
        .invoke_handler(commands::handlers())
        .run(tauri::generate_context!())
        .expect("failed to run EnigmaSaurus");
}
