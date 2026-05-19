use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc,
};
use std::time::Duration;

use tauri::{
    image::Image,
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager, WebviewUrl, WebviewWindowBuilder,
};
use tokio::sync::Mutex;
use tracing::{error, info};
use voxctr_config::Config;
use voxctr_routing::{load_bindings, load_targets, config_dir, OutputTargetRouter};

use crate::{
    commands::*,
    state::{AppState, HistoryEntry},
};

mod commands;
mod state;

// ── Tauri app entry point ─────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialise logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "voxctr=info".parse().unwrap()),
        )
        .init();

    let config = Config::load();
    let cfg_data = Arc::new(config.data.clone());
    let config = Arc::new(Mutex::new(config));

    let cdir = config_dir();
    let targets = load_targets(&cdir).unwrap_or_default();
    let bindings = load_bindings(&cdir).unwrap_or_default();

    let router = Arc::new(Mutex::new(OutputTargetRouter::new(targets.clone())));

    let app_state = Arc::new(AppState {
        config: config.clone(),
        router: router.clone(),
        recording: Arc::new(AtomicBool::new(false)),
        speaking: Arc::new(AtomicBool::new(false)),
        word_count: Arc::new(AtomicU32::new(0)),
        last_text: Arc::new(Mutex::new(String::new())),
        history: Arc::new(Mutex::new(Vec::new())),
    });

    // ── Audio pipeline ────────────────────────────────────────────────────────
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<voxctr_audio::AudioChunk>(64);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<voxctr_inference::InferenceOutput>(32);

    {
        let audio_cfg = cfg_data.audio.clone();
        let recorder = voxctr_audio::AudioRecorder::new(audio_cfg);
        // The recording flag is shared so the audio thread knows when to send chunks
        let recording_flag = app_state.recording.clone();
        let _ = recorder.run(audio_tx, None);
    }

    // Inference worker
    voxctr_inference::run_worker(cfg_data.clone(), audio_rx, text_tx.clone());

    // ── TTS ───────────────────────────────────────────────────────────────────
    let tts_handle = if cfg_data.tts.enabled {
        Some(voxctr_tts::TtsEngineWorker::start(cfg_data.tts.clone()))
    } else {
        None
    };

    // ── Hotkey listener ───────────────────────────────────────────────────────
    let (gesture_tx, mut gesture_rx) = voxctr_hotkeys::channel();
    let _listener = voxctr_hotkeys::start_listener(
        bindings,
        gesture_tx,
        cfg_data.audio.evdev_device.clone(),
    );

    let state_for_gesture = app_state.clone();
    tokio::spawn(async move {
        while let Some(event) = gesture_rx.recv().await {
            use voxctr_hotkeys::GestureKind;
            match event.kind {
                GestureKind::Start => state_for_gesture.set_recording(true),
                GestureKind::Stop => state_for_gesture.set_recording(false),
            }
            // TODO: forward gesture target_id to audio/inference chain
        }
    });

    // ── Text delivery: inference → router → injection ─────────────────────────
    {
        let state = app_state.clone();
        let show_notif = cfg_data.features.show_notification;
        std::thread::spawn(move || {
            while let Ok(output) = text_rx.recv() {
                let words = output.text.split_whitespace().count() as u32;
                state.increment_words(words);

                // Inject into focused window (default target)
                let text = output.text.clone();
                tokio::runtime::Handle::try_current()
                    .map(|h| {
                        h.spawn(async move {
                            if let Err(e) = voxctr_inject::inject_text(&text).await {
                                error!("Inject error: {e}");
                            }
                        });
                    })
                    .ok();

                if show_notif {
                    voxctr_inject::show_notification("VoxCtr", &output.text);
                }

                // Push to history (run in blocking context)
                let state2 = state.clone();
                let entry = HistoryEntry {
                    text: output.text,
                    target_id: output.target_id,
                    timestamp: chrono::Utc::now().to_rfc3339(),
                    inference_ms: output.inference_ms,
                };
                tokio::runtime::Handle::try_current()
                    .map(|h| {
                        h.spawn(async move {
                            let mut hist = state2.history.lock().await;
                            hist.insert(0, entry);
                            if hist.len() > 500 {
                                hist.truncate(500);
                            }
                        });
                    })
                    .ok();
            }
        });
    }

    // ── DBus service (Linux) ──────────────────────────────────────────────────
    #[cfg(target_os = "linux")]
    {
        let dbus_state = Arc::new(Mutex::new(voxctr_dbus::AppState::default()));
        let (start_tx, mut start_rx) = tokio::sync::mpsc::channel::<()>(4);
        let (stop_tx, mut stop_rx) = tokio::sync::mpsc::channel::<()>(4);
        let app_state_dbus = app_state.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    Some(_) = start_rx.recv() => { app_state_dbus.set_recording(true); }
                    Some(_) = stop_rx.recv() => { app_state_dbus.set_recording(false); }
                }
            }
        });
        tokio::spawn(async move {
            if let Err(e) = voxctr_dbus::start_service(dbus_state, start_tx, stop_tx).await {
                error!("DBus service error: {e}");
            }
        });
    }

    // ── Build Tauri app ───────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .setup(|app| {
            // ── System tray ───────────────────────────────────────────────────
            let _tray = TrayIconBuilder::new()
                .tooltip("VoxCtr")
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("settings") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // ── Overlay window (transparent, always-on-top) ───────────────────
            WebviewWindowBuilder::new(app, "overlay", WebviewUrl::App("/overlay".into()))
                .title("VoxCtr Overlay")
                .decorations(false)
                .transparent(true)
                .always_on_top(true)
                .skip_taskbar(true)
                .inner_size(320.0, 120.0)
                .resizable(false)
                .visible(false)
                .build()?;

            // ── Settings window ───────────────────────────────────────────────
            WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("/settings".into()))
                .title("VoxCtr Settings")
                .inner_size(840.0, 640.0)
                .min_inner_size(700.0, 500.0)
                .visible(false)
                .build()?;

            // ── History window ────────────────────────────────────────────────
            WebviewWindowBuilder::new(app, "history", WebviewUrl::App("/history".into()))
                .title("VoxCtr — Transcript History")
                .inner_size(600.0, 500.0)
                .visible(false)
                .build()?;

            // Emit periodic status updates to all windows
            let state_for_ticker = app_state.clone();
            let handle = app.handle().clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(200));
                loop {
                    interval.tick().await;
                    let payload = serde_json::json!({
                        "recording": state_for_ticker.is_recording(),
                        "speaking": state_for_ticker.is_speaking(),
                        "word_count": state_for_ticker.total_words(),
                    });
                    let _ = handle.emit("status-tick", payload);
                }
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            start_recording,
            stop_recording,
            toggle_recording,
            get_config,
            save_config,
            get_targets,
            save_targets,
            get_bindings,
            save_bindings,
            get_history,
            clear_history,
            speak_text,
            show_overlay,
            hide_overlay,
            list_audio_devices,
        ])
        .run(tauri::generate_context!())
        .expect("error running Tauri application");
}
