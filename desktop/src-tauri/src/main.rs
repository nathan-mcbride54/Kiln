#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    kiln_tauri::run();
}
