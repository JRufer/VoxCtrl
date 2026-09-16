// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn init_x11_threads() {
    unsafe {
        if let Ok(filename) = std::ffi::CString::new("libX11.so.6") {
            let handle = libc::dlopen(filename.as_ptr(), libc::RTLD_LAZY | libc::RTLD_GLOBAL);
            if !handle.is_null() {
                if let Ok(symbol) = std::ffi::CString::new("XInitThreads") {
                    let ptr = libc::dlsym(handle, symbol.as_ptr());
                    if !ptr.is_null() {
                        let x_init_threads: unsafe extern "C" fn() -> std::os::raw::c_int = std::mem::transmute(ptr);
                        x_init_threads();
                    }
                }
            }
        }
    }
}

fn main() {
    // Initialize X11 thread safety BEFORE any GTK/WebKit/X11 windows open.
    #[cfg(target_os = "linux")]
    {
        init_x11_threads();
        // Disable DMA-BUF renderer to fix black transparent background on Nvidia/proprietary drivers.
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        // Disable WebKitGTK's accelerated compositor: layer-promoted elements
        // (anything animating opacity/transform) otherwise blend into the
        // transparent window with the wrong alpha, showing up as a blurry,
        // semi-opaque smear for the duration of the overlay's load/unload
        // animation. See lib.rs::run() for the full explanation.
        std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
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
