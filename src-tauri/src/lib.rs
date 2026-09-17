// VoxCtrl Tauri Application Core
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};
#[cfg(target_os = "linux")]
use tauri::Manager;
use tokio::sync::Mutex;
use voxctrl_config::Config;
use voxctrl_routing::{config_dir, load_bindings, load_targets, OutputTargetRouter};

use crate::commands::*;
use crate::state::AppState;

mod bug_report;
mod commands;
mod custom_overlays;
mod installer;
mod host_env;
mod mint_shortcuts;
mod pipeline;
pub mod render_env;
mod services;
mod startup_log;
mod state;
mod stop_key;
mod tray;
mod updater;
mod window;

#[cfg(test)]
mod tests;

pub use window::{
    get_app_handle, set_app_handle, setup_blocker, show_and_focus_window, show_setup_window,
    SETUP_WINDOW,
};

pub fn run_cli_installer() -> Result<(), String> {
    crate::installer::run_cli_installer()
}

/// The commit this binary was built from, or `"unknown"` for a build that did
/// not set `VOXCTRL_BUILD_SHA` (a bare `cargo build`, say).
///
/// Set by `.github/workflows/release.yml` and `build_appimage.sh`; see
/// `build.rs` for why it needs a `rerun-if-env-changed`.
pub const BUILD_SHA: &str = match option_env!("VOXCTRL_BUILD_SHA") {
    Some(sha) => sha,
    None => "unknown",
};

/// What this build is, as one line: the version, and the commit behind it.
///
/// Reported by `--version` and written to the startup log. A packaged build
/// otherwise has no way to say which source it came from, and answering
/// "does this build actually contain the fix?" by guessing has repeatedly
/// produced confident conclusions from builds that predated the change being
/// tested.
pub fn version_string() -> String {
    format_version(env!("CARGO_PKG_VERSION"), BUILD_SHA)
}

/// Split out from [`version_string`] so the formatting is testable without
/// rebuilding with a particular `VOXCTRL_BUILD_SHA`.
///
/// The SHA is shortened so CI and local builds read the same: the workflow
/// passes the full 40-character `github.sha`, `build_appimage.sh` passes a
/// short one. A `-dirty` suffix survives, since that is the part worth
/// noticing.
fn format_version(version: &str, build_sha: &str) -> String {
    let sha = match build_sha.split_once('-') {
        Some((hash, suffix)) => format!("{}-{}", short_sha(hash), suffix),
        None => short_sha(build_sha).to_string(),
    };
    format!("VoxCtrl {version} (build {sha})")
}

fn short_sha(sha: &str) -> &str {
    // Only shorten something that actually looks like a hash, so "unknown"
    // stays readable rather than becoming "unknow".
    if sha.len() > 7 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
        &sha[..7]
    } else {
        sha
    }
}

#[cfg(test)]
mod version_tests {
    use super::format_version;

    /// The workflow passes `github.sha`, which is the full 40 characters.
    #[test]
    fn a_ci_sha_is_shortened() {
        assert_eq!(
            format_version("0.6.1", "57375d4d5882d8222b7a978190a058795acb9cee"),
            "VoxCtrl 0.6.1 (build 57375d4)"
        );
    }

    /// `build_appimage.sh` already passes a short one; it must not be
    /// shortened twice or reported differently from the same CI build.
    #[test]
    fn a_local_sha_is_left_alone() {
        assert_eq!(format_version("0.6.1", "57375d4"), "VoxCtrl 0.6.1 (build 57375d4)");
    }

    /// A build made with uncommitted changes has to say so — that is the
    /// case where "which code is this?" matters most.
    #[test]
    fn a_dirty_build_keeps_its_marker() {
        assert_eq!(
            format_version("0.6.1", "57375d4-dirty"),
            "VoxCtrl 0.6.1 (build 57375d4-dirty)"
        );
        assert_eq!(
            format_version("0.6.1", "57375d4d5882d8222b7a978190a058795acb9cee-dirty"),
            "VoxCtrl 0.6.1 (build 57375d4-dirty)"
        );
    }

    /// A build with no SHA set should say so plainly rather than print a
    /// truncated word that looks like a hash.
    #[test]
    fn an_unstamped_build_stays_readable() {
        assert_eq!(format_version("0.6.1", "unknown"), "VoxCtrl 0.6.1 (build unknown)");
    }
}

#[cfg(test)]
pub mod test_utils {
    use std::sync::{Mutex, OnceLock};
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    pub fn get_env_lock() -> &'static Mutex<()> {
        ENV_LOCK.get_or_init(|| Mutex::new(()))
    }
}

// ── Tauri app entry point ─────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Whether these command-line arguments ask for the setup wizard.
///
/// Accepted in a few spellings because this is the flag someone reaches for
/// when the app is already configured and they want to see setup again; making
/// them guess the exact word would defeat the point.
pub fn wants_setup_wizard(args: &[String]) -> bool {
    args.iter().any(|a| {
        matches!(
            a.as_str(),
            "--setup" | "--wizard" | "--setup-wizard" | "--first-run"
        )
    })
}

pub fn run() {
    #[cfg(target_os = "linux")]
    {
        // Graphics-stack defaults. `main()` applies these first, before
        // anything at all has run; this covers the paths that reach `run()`
        // without going through it. See `render_env`.
        crate::render_env::apply();

        // The dictation overlay needs a window that can set its own absolute
        // position and reliably stay above other windows. A native Wayland
        // toplevel can do neither — position and stacking are compositor
        // policy, not something a client gets to ask for. Forcing the app
        // through XWayland gets back the X11 behavior this depends on
        // (absolute positioning, `_NET_WM_STATE_ABOVE`), at the cost of
        // native Wayland features (e.g. fractional scaling) for the whole
        // app, not just the overlay — GTK's backend is chosen once,
        // process-wide, at init, so there's no way to select it per-window.
        // Only forced when a Wayland session with a reachable X server
        // (XWayland) is actually detected.
        if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() {
            eprintln!(
                "Wayland session detected: forcing GDK_BACKEND=x11 (via XWayland) so the \
                 dictation overlay can position itself and stay always-on-top"
            );
            std::env::set_var("GDK_BACKEND", "x11");
        }

        // Suppress libayatana-appindicator deprecation warnings by registering a dummy log handler
        unsafe {
            extern "C" {
                fn g_log_set_handler(
                    log_domain: *const std::os::raw::c_char,
                    log_levels: std::os::raw::c_int,
                    log_func: Option<
                        unsafe extern "C" fn(
                            *const std::os::raw::c_char,
                            std::os::raw::c_int,
                            *const std::os::raw::c_char,
                            *mut std::os::raw::c_void,
                        ),
                    >,
                    user_data: *mut std::os::raw::c_void,
                ) -> std::os::raw::c_uint;
            }

            unsafe extern "C" fn dummy_log_handler(
                _log_domain: *const std::os::raw::c_char,
                _log_levels: std::os::raw::c_int,
                _message: *const std::os::raw::c_char,
                _user_data: *mut std::os::raw::c_void,
            ) {
            }

            let domain = b"libayatana-appindicator\0".as_ptr() as *const std::os::raw::c_char;
            g_log_set_handler(domain, 16, Some(dummy_log_handler), std::ptr::null_mut());
        }
    }

    // Initialise logging (console + special warning-free/privacy-safe startup and error file log)
    use tracing_subscriber::prelude::*;
    let local_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voxctrl");
    let _ = std::fs::create_dir_all(&local_dir);
    let log_path = local_dir.join("startup_errors.log");

    let file_layer = match startup_log::StartupErrorLayer::new(log_path) {
        Ok(layer) => Some(layer),
        Err(e) => {
            eprintln!("Failed to initialize startup error log file: {e}");
            None
        }
    };

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "voxctrl=info".parse().unwrap());

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer());

    if let Some(fl) = file_layer {
        let _ = registry.with(fl).try_init();
    } else {
        let _ = registry.try_init();
    }

    // First line in every log and every bug report: which build this is. A
    // report or a reproduction attempt against an unidentified build has
    // repeatedly turned out to be against one that predated the change in
    // question.
    tracing::info!("{}", version_string());

    // Make sure the documented Custom/ overlay example exists — see
    // refresh_bundled_example's doc comment: it never touches Custom/ once
    // it exists, edited or not, only recreating it if the folder is deleted.
    // Nothing else in the overlays folder is touched.
    custom_overlays::refresh_bundled_example();

    let config = Config::load();

    // Log the sanitized configuration parameters at startup
    tracing::info!("=== System Startup Config ===");
    tracing::info!("Backend choice: {:?}", config.data.engine.backend);
    tracing::info!("Whisper model size: {}", config.data.engine.whisper_cpp.model_size);
    tracing::info!("Whisper device: {}", config.data.engine.whisper_cpp.device);
    tracing::info!("Whisper threads: {}", config.data.engine.whisper_cpp.threads);
    // What the build can offload, as opposed to what the config asks for. The
    // two differ more often than they look like they should: the Vulkan build
    // accelerates whisper.cpp and nothing else.
    tracing::info!(
        "GPU support in this build — whisper.cpp: {}, Moonshine: {}, Parakeet: {}",
        voxctrl_inference::whisper_gpu_backend().unwrap_or("none (CPU)"),
        voxctrl_inference::moonshine_gpu_backend().unwrap_or("none (CPU)"),
        voxctrl_inference::parakeet_gpu_backend().unwrap_or("none (CPU)"),
    );
    tracing::info!("Moonshine model size: {}", config.data.engine.moonshine.model_size);
    tracing::info!("Moonshine language: {}", config.data.engine.moonshine.language);
    tracing::info!("VAD threshold: {}", config.data.audio.vad_threshold);
    tracing::info!("Noise suppression: {}", config.data.audio.noise_suppression);
    tracing::info!("Input device index: {:?}", config.data.audio.input_device_index);
    tracing::info!("Gain: {}", config.data.audio.gain);
    tracing::info!("Dynamic stream: {}", config.data.audio.dynamic_stream);
    tracing::info!("TTS enabled: {}", config.data.tts.enabled);
    tracing::info!("TTS engine: {:?}", config.data.tts.engine);
    tracing::info!("TTS voice: {}", config.data.tts.voice);
    tracing::info!("TTS speed: {}", config.data.tts.speed);
    tracing::info!("TTS GPU: {}", config.data.tts.gpu);
    tracing::info!("Pocket-TTS voice: {}", config.data.tts.pocket_tts.voice);
    tracing::info!("Pocket-TTS prewarm: {}", config.data.tts.pocket_tts.prewarm);
    tracing::info!(
        "HuggingFace token: {}",
        if voxctrl_tts::hf_token_from_env().is_some() {
            "from HF_TOKEN"
        } else if config.data.tts.hf_token.is_some() {
            "from config"
        } else {
            "not set"
        }
    );
    tracing::info!("MCP enabled: {}", config.data.mcp.server_enabled);
    tracing::info!("MCP record timeout: {}", config.data.mcp.record_timeout);
    tracing::info!("=============================");

    let cfg_data = Arc::new(config.data.clone());
    let config = Arc::new(Mutex::new(config));

    let cdir = config_dir();
    let targets = load_targets(&cdir).unwrap_or_default();
    let bindings = load_bindings(&cdir).unwrap_or_default();

    let router = Arc::new(OutputTargetRouter::new(targets.clone()));

    // ── Audio & Inference pipelines ──────────────────────────────────────────
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<voxctrl_audio::AudioChunk>(64);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<voxctrl_inference::InferenceOutput>(32);
    let (inference_tx, inference_rx) =
        crossbeam_channel::bounded::<voxctrl_inference::InferenceRequest>(4);
    let (inference_cfg_tx, inference_cfg_rx) =
        crossbeam_channel::unbounded::<Arc<voxctrl_config::AppConfig>>();
    let (overlay_tx, overlay_rx) = crossbeam_channel::unbounded::<String>();
    // One pending nudge is all the capture supervisor needs: it re-reads every
    // flag when it wakes, so a second nudge would tell it nothing new.
    let (audio_wake_tx, audio_wake_rx) = crossbeam_channel::bounded::<()>(1);

    let hotkey_health = Arc::new(voxctrl_hotkeys::ListenerHealth::default());

    let app_state = Arc::new(AppState {
        config: config.clone(),
        router: router.clone(),
        recording: Arc::new(AtomicBool::new(false)),
        processing: Arc::new(AtomicBool::new(false)),
        speaking: Arc::new(AtomicBool::new(false)),
        overlay_enabled: Arc::new(AtomicBool::new(cfg_data.ui.show_overlay)),
        mcp_recording: Arc::new(AtomicBool::new(false)),
        command_overlay_until: Arc::new(std::sync::Mutex::new(None)),
        hotkeys_inhibited: Arc::new(AtomicBool::new(false)),
        audio_ready: Arc::new(AtomicBool::new(false)),
        dynamic_stream: Arc::new(AtomicBool::new(cfg_data.audio.dynamic_stream)),
        monitoring: Arc::new(AtomicBool::new(false)),
        input_device_index: Arc::new(AtomicU32::new(
            cfg_data.audio.input_device_index.unwrap_or(u32::MAX),
        )),
        gain: Arc::new(AtomicU32::new(cfg_data.audio.gain.to_bits())),
        noise_suppression: Arc::new(AtomicBool::new(cfg_data.audio.noise_suppression)),
        word_count: Arc::new(AtomicU32::new(0)),
        last_text: Arc::new(Mutex::new(String::new())),
        last_text_version: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        active_target: Arc::new(Mutex::new("default".to_string())),
        active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
        active_binding_id: Arc::new(Mutex::new(String::new())),
        targets: Arc::new(Mutex::new(targets.clone())),
        audio_tx: audio_tx.clone(),
        audio_wake: audio_wake_tx,
        inference_config_tx: inference_cfg_tx,
        tts_handle: Arc::new(Mutex::new(None)),
        active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        stop_key_held: Arc::new(AtomicBool::new(false)),
        speaking_tx: Arc::new(std::sync::OnceLock::new()),
        hotkey_reloader: Arc::new(Mutex::new(None)),
        hotkey_gesture_tx: Arc::new(Mutex::new(None)),
        hotkey_health: hotkey_health.clone(),
        overlay_tx: overlay_tx.clone(),
        pending_update: Arc::new(Mutex::new(None)),
        updating: Arc::new(AtomicBool::new(false)),
    });

    let (audio_level_tx, audio_level_rx) = crossbeam_channel::bounded::<f32>(128);

    {
        let audio_cfg = cfg_data.audio.clone();
        let recorder = voxctrl_audio::AudioRecorder::new(
            audio_cfg,
            app_state.recording.clone(),
            app_state.monitoring.clone(),
            app_state.dynamic_stream.clone(),
            app_state.input_device_index.clone(),
            app_state.gain.clone(),
            app_state.noise_suppression.clone(),
        )
        .with_wake(audio_wake_rx);
        let _ = recorder.run(
            audio_tx,
            Some(audio_level_tx),
            Some(app_state.audio_ready.clone()),
        );
    }

    // Audio chunk coordinator thread
    let rt_handle = tokio::runtime::Handle::current();
    pipeline::spawn_audio_coordinator(
        app_state.clone(),
        audio_rx,
        inference_tx,
        text_tx.clone(),
        rt_handle.clone(),
    );

    // Inference worker with live config reloading
    voxctrl_inference::run_worker_with_config(
        cfg_data.clone(),
        inference_rx,
        text_tx.clone(),
        inference_cfg_rx,
    );

    // TTS initial worker
    let _tts_handle = if cfg_data.tts.enabled {
        Some(voxctrl_tts::TtsEngineWorker::start(
            cfg_data.tts.clone(),
            cfg_data.features.custom_vocabulary.clone(),
            None,
            None,
            None,
        ))
    } else {
        None
    };

    let state_for_tts = app_state.clone();
    let tts_handle_clone = _tts_handle.clone();
    tokio::spawn(async move {
        {
            let mut handle = state_for_tts.tts_handle.lock().await;
            *handle = tts_handle_clone.clone();
        }
        if let Some(tts) = tts_handle_clone {
            state_for_tts.spawn_fifo_responders(tts).await;
        }
    });

    // Setup desktop integration (launcher and icon) before initializing hotkey listeners,
    // so xdg-desktop-portal can resolve the `ai.voxctrl.app` AppID against an installed .desktop file.
    #[cfg(target_os = "linux")]
    {
        if let Err(e) = crate::installer::setup_desktop_integration() {
            tracing::warn!("Failed to setup desktop integration at startup: {e}");
        }
    }

    // Hotkey listener & bindings
    //
    // The stop key is not in this set. Where the desktop owns the key grab a
    // standing registration on bare Escape would take the key from every other
    // app, so `stop_key`'s arbiter adds it — here, immediately, on the backends
    // that grab nothing, and only while VoxCtrl speaks on the ones that do.
    let mut all_bindings = bindings;
    let initial_grab =
        crate::stop_key::stop_key_grab(hotkey_health.backend(), &cfg_data.tts.stop_key);
    if crate::stop_key::stop_key_wanted(initial_grab, app_state.is_speaking()) {
        app_state
            .stop_key_held
            .store(true, std::sync::atomic::Ordering::SeqCst);
        all_bindings.push(crate::stop_key::stop_binding(cfg_data.tts.stop_key.clone()));
    }

    let (gesture_tx, gesture_rx) = voxctrl_hotkeys::channel();
    let listener = voxctrl_hotkeys::start_listener(
        all_bindings,
        gesture_tx.clone(),
        cfg_data.audio.evdev_device.clone(),
        hotkey_health.clone(),
    );

    crate::stop_key::spawn(app_state.clone());

    let state_for_gesture = app_state.clone();
    let gesture_tx_for_state = gesture_tx.clone();
    tokio::spawn(async move {
        let mut reloader = state_for_gesture.hotkey_reloader.lock().await;
        *reloader = Some(listener.reloader_tx);
        let mut gtx = state_for_gesture.hotkey_gesture_tx.lock().await;
        *gtx = Some(gesture_tx_for_state);
    });

    pipeline::spawn_hotkey_gesture_handler(app_state.clone(), gesture_rx);

    // Text delivery worker
    pipeline::spawn_text_delivery_worker(app_state.clone(), text_rx, rt_handle);

    // DBus service
    #[cfg(target_os = "linux")]
    services::start_dbus_service(app_state.clone());

    // MCP Server
    if cfg_data.mcp.server_enabled {
        services::start_mcp_server(app_state.clone());
    }

    // ── Build Tauri app ───────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            tracing::info!("Single instance trigger: argv={:?}, cwd={:?}", argv, cwd);
            // `voxctrl --setup` while the app is already running is a request
            // to see the wizard, not to raise Settings. Without this the flag
            // would appear to do nothing at all on the second launch, which is
            // the launch a user testing it is most likely to make.
            if wants_setup_wizard(&argv) {
                if let Err(e) = crate::window::open_wizard_window(app) {
                    tracing::error!("Could not open the setup wizard: {e}");
                }
                return;
            }
            if let Err(e) = crate::window::open_settings_window(app) {
                tracing::error!("Could not open Settings: {e}");
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .setup(move |app| {
            set_app_handle(app.handle().clone());

            // Set window icon programmatically on Linux/Wayland
            #[cfg(target_os = "linux")]
            {
                let _ = crate::installer::setup_desktop_integration();
                let icon_bytes = include_bytes!("../icons/128x128.png");
                if let Ok(icon) = tauri::image::Image::from_bytes(icon_bytes) {
                    for window in app.webview_windows().values() {
                        let _ = window.set_icon(icon.clone());
                    }
                }
            }

            // Re-initialize TTS worker with event emitter callbacks
            services::setup_tts_and_fifos(&app.handle(), app_state.clone());

            // Register Speak target callback
            services::register_speak_target(&app.handle());

            // Register Command trigger target callback
            services::register_command_trigger_target(&app.handle());

            // Setup watcher for hotkey permissions
            #[cfg(target_os = "linux")]
            pipeline::spawn_setup_watcher(app.handle().clone(), app_state.hotkey_health.clone());

            // Forward audio levels to the settings window and the overlay
            pipeline::spawn_audio_level_forwarder(
                app.handle().clone(),
                app_state.clone(),
                audio_level_rx,
            );

            // Build the overlay window now rather than on the first
            // dictation. Constructing it costs one brief flash of an empty
            // window — a webview is mapped before it has loaded `/overlay`
            // and painted — and that cost has to land somewhere. Paid here it
            // lands during startup, alongside everything else the app is
            // doing before the user has asked for anything; paid on the first
            // keybind press it lands in the middle of the thing the user is
            // watching. `tray::spawn_status_ticker` still builds it if this
            // fails, so a failure here delays the overlay rather than losing
            // it.
            if let Err(e) = crate::window::open_overlay_window(
                app.handle(),
                &cfg_data.ui.overlay_position,
                &cfg_data.ui.overlay_monitor,
            ) {
                tracing::warn!("Could not pre-build the dictation overlay: {e}");
            }

            // Setup system tray
            let _tray = tray::create_tray(app)?;
            tray::sync_tts_memory_item(app.handle().clone(), app_state.clone());

            let record_on_icon =
                tauri::image::Image::from_bytes(include_bytes!("../../assets/record_on.png"))
                    .expect("Failed to load record_on icon");
            let record_off_icon =
                tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
                    .expect("Failed to load record_off icon");
            let processing_frames = [
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_1.png"))
                    .expect("Failed to load processing_1 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_2.png"))
                    .expect("Failed to load processing_2 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_3.png"))
                    .expect("Failed to load processing_3 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_4.png"))
                    .expect("Failed to load processing_4 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_5.png"))
                    .expect("Failed to load processing_5 icon"),
                tauri::image::Image::from_bytes(include_bytes!("../../assets/processing_6.png"))
                    .expect("Failed to load processing_6 icon"),
            ];

            // ── Dictation overlay ────────────────────────────────────────────────
            // window::open_overlay_window builds the transparent,
            // click-through, always-on-top WebviewWindow that renders the
            // `/overlay` route (src/lib/Overlay/Overlay.svelte) — see that
            // function's doc comment and docs/overlays.md for how it's put
            // together. It is built once, above, and kept for the session;
            // tray::spawn_status_ticker (below) builds it there instead if
            // that failed, and nothing destroys it.
            let overlay_handle = app.handle().clone();
            // overlay_tx carries position updates (sent whenever
            // config.ui.overlay_position / overlay_monitor change — see
            // commands.rs's save_config and tray.rs's config-change ticker) and
            // status/audio-level messages the audio-level forwarder also emits
            // as Tauri events (status-tick / audio-level) that the frontend
            // consumes directly; only the position updates need applying here.
            std::thread::spawn(move || {
                while let Ok(msg) = overlay_rx.recv() {
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&msg) else {
                        continue;
                    };
                    if value.get("type").and_then(|t| t.as_str()) != Some("position") {
                        continue;
                    }
                    let position = value.get("position").and_then(|v| v.as_str()).unwrap_or("");
                    let monitor = value.get("monitor").and_then(|v| v.as_str()).unwrap_or("primary");
                    crate::window::reposition_overlay(&overlay_handle, position, monitor);
                }
            });

            // Auto download speech model if needed. Skipped entirely when the
            // wizard was asked for: it is about to ask which model the user
            // wants, and fetching a different one behind its back would waste
            // the download and confuse the step.
            let forced_wizard = wants_setup_wizard(&std::env::args().collect::<Vec<_>>());
            if forced_wizard {
                if let Err(e) = crate::window::open_wizard_window(&app.handle().clone()) {
                    tracing::error!("Could not open the setup wizard: {e}");
                }
            } else {
                services::auto_download_speech_model_if_needed(app, &cfg_data);
            }

            // Emit periodic status updates to all windows and animate tray
            tray::spawn_status_ticker(
                app.handle().clone(),
                app_state.clone(),
                record_on_icon,
                record_off_icon,
                processing_frames,
            );

            // Look for a new release, unless the user has turned that off or
            // is standing in front of the setup wizard — a fresh install is on
            // the latest version anyway, and an update dialog landing on top of
            // step one of setup is nobody's idea of a first impression.
            if !forced_wizard && cfg_data.ui.setup_completed {
                updater::spawn_launch_check(app.handle().clone(), app_state.clone());
            }

            startup_log::STARTUP_COMPLETE.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            get_available_monitors,
            start_recording,
            stop_recording,
            toggle_recording,
            get_config,
            save_config,
            get_targets,
            save_targets,
            get_bindings,
            save_bindings,
            speak_text,
            get_custom_overlays,
            get_custom_overlay,
            overlay_content_ready,
            get_custom_overlays_dir,
            get_cloned_tts_voices_dir,
            list_audio_devices,
            start_monitoring_audio,
            stop_monitoring_audio,
            check_voice_downloaded,
            download_voice,
            check_breeze_tts_2_ready,
            preview_timestamp_format,
            hf_token_env,
            download_breeze_tts_2,
            check_vox_cpm_2_ready,
            check_vox_cpm_2_downloaded,
            download_vox_cpm_2,
            check_pocket_tts_ready,
            download_pocket_tts,
            list_pocket_tts_voices,
            inflect_micro_available,
            check_inflect_micro_downloaded,
            download_inflect_micro,
            inflect_micro_inspect,
            check_model_downloaded,
            download_model,
            moonshine_available,
            check_moonshine_downloaded,
            download_moonshine_model,
            parakeet_available,
            check_parakeet_downloaded,
            download_parakeet_model,
            check_s1_mini_downloaded,
            download_s1_mini_model,
            check_directory_exists,
            test_openai,
            test_remote_stt,
            cuda_enabled,
            accelerator_support,
            check_hotkey_status,
            check_hotkey_keys,
            retry_portal_shortcuts,
            open_shortcut_settings,
            register_mint_shortcut,
            approve_shortcuts,
            install_system_integration,
            get_setup_status,
            finish_setup_wizard,
            open_setup_wizard,
            download_configured_model,
            open_settings_tab,
            stop_tts,
            reset_chat_conversation,
            test_chat_target,
            set_hotkeys_inhibited,
            updater::check_for_update,
            updater::get_pending_update,
            updater::install_update,
            updater::skip_update_version,
            updater::set_update_auto_check,
            updater::dismiss_update,
            updater::open_update_window,
            bug_report::bug_report_context,
            bug_report::preview_bug_report,
            bug_report::submit_bug_report,
            bug_report::save_bug_report,
            bug_report::open_external_url,
            bug_report::open_report_folder,
            bug_report::send_bug_report_email,
            bug_report::suggested_bug_report_filename,
            bug_report::reset_bug_report_identity,
        ])
        .build(tauri::generate_context!())
        .expect("error building Tauri application")
        .run(|_app, event| {
            // Windows are ordinary windows: the close button closes them, and
            // every entry point rebuilds one when it is gone. That makes the
            // last window closing look to Tauri like the app should exit, which
            // for a tray app it must not — dictation carries on with nothing on
            // screen. An explicit quit carries an exit code, so the tray's Quit
            // item still works.
            if let tauri::RunEvent::ExitRequested { code, api, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}
