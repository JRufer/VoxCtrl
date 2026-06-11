// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Set Linux WebKit environment variables BEFORE GTK/WebKit initializes.
    // These MUST be set here in main() — setting them inside lib.rs::run() is too late
    // because Tauri already sets up the WebKit webview during builder initialization.
    #[cfg(target_os = "linux")]
    {
        // Disable DMA-BUF renderer to fix black transparent background on Nvidia/proprietary drivers.
        // We leave hardware acceleration (compositing) enabled since it is often required for visual transparency.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

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
