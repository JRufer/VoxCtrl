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

/// `voxctrl --server [options]`: run the headless speech server instead of
/// the app. Returns its exit code, or `None` when that was not asked for.
///
/// Checked before anything graphical is touched, so it works over SSH and on
/// a machine with no display: no window, tray icon or audio device is opened.
fn run_as_server(args: &[String]) -> Option<i32> {
    let options = server_options(args)?;
    #[cfg(target_os = "linux")]
    {
        // Name the program the way it was started, so the server's own
        // messages ("run … --pairing-code") can be typed as shown.
        let started_as = std::env::var("APPIMAGE")
            .ok()
            .or_else(|| args.first().cloned())
            .and_then(|path| std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "voxctrl".into());
        Some(voxctrl_server::main_with(&format!("{started_as} --server"), options.iter().cloned()))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = options;
        eprintln!("Running as a server is available on Linux only.");
        Some(2)
    }
}

/// The server's own options when the command line asks for server mode:
/// everything after a leading `--server`. `--server` anywhere else is not it,
/// so the app's other arguments are never mistaken for it.
fn server_options(args: &[String]) -> Option<&[String]> {
    match args.get(1).map(String::as_str) {
        Some("--server" | "server") => Some(&args[2..]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::server_options;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn server_mode_is_a_leading_server_flag_and_the_rest_are_its_options() {
        let line = args(&["voxctrl", "--server", "--bind", "0.0.0.0:9000"]);
        assert_eq!(server_options(&line), Some(&line[2..]));
        assert_eq!(server_options(&args(&["voxctrl", "server"])), Some(&[][..]));
    }

    #[test]
    fn the_app_starts_normally_otherwise() {
        assert_eq!(server_options(&args(&["voxctrl"])), None);
        assert_eq!(server_options(&args(&["voxctrl", "--version"])), None);
        // Only the first argument counts: this is the app, started minimised.
        assert_eq!(server_options(&args(&["voxctrl", "--minimized", "--server"])), None);
    }
}

fn main() {
    if let Some(code) = run_as_server(&std::env::args().collect::<Vec<_>>()) {
        std::process::exit(code);
    }

    // Initialize X11 thread safety BEFORE any GTK/WebKit/X11 windows open.
    #[cfg(target_os = "linux")]
    {
        init_x11_threads();
        // Graphics-stack defaults, applied here because GTK, GDK and WebKit
        // each read this environment once, when they initialise — which is
        // after this point and before anything else the app does. See
        // voxctrl_app_lib::render_env.
        voxctrl_app_lib::render_env::apply();
    }

    let args: Vec<String> = std::env::args().collect();

    // Answered before anything else starts up, so it works on a machine where
    // the app itself cannot run. This is how a packaged build is asked which
    // commit it came from — without it, "is the fix actually in this
    // AppImage?" can only be guessed at.
    if args.len() > 1 && matches!(args[1].as_str(), "--version" | "-V" | "version") {
        println!("{}", voxctrl_app_lib::version_string());
        std::process::exit(0);
    }

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
