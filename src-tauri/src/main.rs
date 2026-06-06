// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--install" || args[1] == "install") {
        if let Err(e) = voxctrl_app_lib::run_cli_installer() {
            eprintln!("Installation failed: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }

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
