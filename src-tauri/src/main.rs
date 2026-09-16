// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// A momentary, genuinely separate Wayland client, spawned by
/// `window::flush_compositor_frame` to unstick a frozen overlay window.
///
/// Confirmed behavior: only *launching a new application* clears a frozen
/// overlay on KDE Wayland — minimizing/restoring an already-running one
/// does not. Every in-process attempt at forcing a repaint (moving the
/// overlay window, resizing it, even mapping an extra window from within
/// VoxCtrl's own already-running process) failed to reproduce that, which
/// points at the compositor throttling frame delivery per *client
/// connection*, not per surface — so nothing the already-connected VoxCtrl
/// process does to its own windows can ever unstick it. Only a fresh
/// process connecting to the Wayland display does.
///
/// This re-execs the VoxCtrl binary itself with a hidden flag so it's a
/// real, separate OS process — a real second Wayland client — rather than
/// another window in the same one. It never reaches `voxctrl_app_lib::run`
/// (so it can't trip the single-instance lock, which would just forward to
/// the running instance and exit without ever opening a display
/// connection): it opens one small GTK window and closes it a moment
/// later, then exits.
#[cfg(target_os = "linux")]
fn run_overlay_flush_helper() {
    use gtk::prelude::*;

    if gtk::init().is_err() {
        return;
    }
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_default_size(1, 1);
    window.set_decorated(false);
    window.move_(-10000, -10000);
    window.show_all();

    // Pump the GTK/Wayland event loop long enough for the window to
    // actually map and the compositor to process it as a new client
    // appearing, then tear it down.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(250);
    while std::time::Instant::now() < deadline {
        while gtk::events_pending() {
            gtk::main_iteration();
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    unsafe {
        window.destroy();
    }
    while gtk::events_pending() {
        gtk::main_iteration();
    }
}

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
    }

    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && (args[1] == "--install" || args[1] == "install") {
        if let Err(e) = voxctrl_app_lib::run_cli_installer() {
            eprintln!("Installation failed: {}", e);
            std::process::exit(1);
        }
        std::process::exit(0);
    }

    #[cfg(target_os = "linux")]
    if args.len() > 1 && args[1] == "--overlay-flush-helper" {
        run_overlay_flush_helper();
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
