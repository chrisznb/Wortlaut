// Keep a second console window from opening on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    wortlaut_tauri_lib::run();
}
