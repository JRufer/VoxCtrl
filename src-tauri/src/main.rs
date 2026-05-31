// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Tokio runtime wraps the Tauri event loop so async tasks work everywhere.
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("build tokio runtime")
        .block_on(async {
            voxctrl_app_lib::run();
        });
}
