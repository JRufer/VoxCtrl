// Force Tauri rebuild with latest voxctr-routing changes
use std::sync::{
    atomic::{AtomicBool, AtomicU32},
    Arc,
};
use std::time::Duration;

use tauri::{
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};
use tokio::sync::Mutex;
use tracing::error;
use voxctr_config::Config;
use voxctr_routing::{load_bindings, load_targets, config_dir, OutputTargetRouter};
use voxctr_mcp::McpCallbacks;

use crate::{
    commands::*,
    state::{AppState, HistoryEntry},
};

mod commands;
mod state;

// Helper to robustly show, unminimize, and focus a window, especially under Linux WMs
fn show_and_focus_window(window: &tauri::WebviewWindow) {
    let w = window.clone();
    tauri::async_runtime::spawn(async move {
        let mut pos = None;
        #[cfg(target_os = "linux")]
        {
            // If the window is already open/visible, we must hide it first and wait a short period
            // (150ms) to allow the Linux window manager (GNOME/Mutter) to fully unmap it.
            // Showing it again triggers a brand new window mapping event, which bypasses Wayland/GNOME's
            // Focus Stealing Prevention, robustly bringing it to the foreground with active keyboard focus.
            if w.is_visible().unwrap_or(false) {
                pos = w.outer_position().ok();
                let _ = w.hide();
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }
        }

        let _ = w.unminimize();
        let _ = w.show();
        #[cfg(target_os = "linux")]
        {
            if let Some(p) = pos {
                let _ = w.set_position(p);
            }
        }
        let _ = w.set_focus();
    });
}

// ── Tauri app entry point ─────────────────────────────────────────────────────

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(target_os = "linux")]
    {
        // Workaround for WebKitGTK blank window/rendering issues due to DMABUF creation failures
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

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

    let router = Arc::new(OutputTargetRouter::new(targets.clone()));

    // ── Audio pipeline ────────────────────────────────────────────────────────
    let (audio_tx, audio_rx) = crossbeam_channel::bounded::<voxctr_audio::AudioChunk>(64);
    let (text_tx, text_rx) = crossbeam_channel::bounded::<voxctr_inference::InferenceOutput>(32);

    let (inference_tx, inference_rx) = crossbeam_channel::bounded::<voxctr_inference::InferenceRequest>(4);

    let app_state = Arc::new(AppState {
        config: config.clone(),
        router: router.clone(),
        recording: Arc::new(AtomicBool::new(false)),
        processing: Arc::new(AtomicBool::new(false)),
        speaking: Arc::new(AtomicBool::new(false)),
        audio_ready: Arc::new(AtomicBool::new(false)),
        dynamic_stream: Arc::new(AtomicBool::new(cfg_data.audio.dynamic_stream)),
        monitoring: Arc::new(AtomicBool::new(false)),
        input_device_index: Arc::new(std::sync::atomic::AtomicU32::new(cfg_data.audio.input_device_index.unwrap_or(u32::MAX))),
        gain: Arc::new(std::sync::atomic::AtomicU32::new(cfg_data.audio.gain.to_bits())),
        word_count: Arc::new(AtomicU32::new(0)),
        last_text: Arc::new(Mutex::new(String::new())),
        active_target: Arc::new(Mutex::new("default".to_string())),
        active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
        targets: Arc::new(Mutex::new(targets.clone())),
        history: Arc::new(Mutex::new(Vec::new())),
        audio_tx: audio_tx.clone(),
        tts_handle: Arc::new(Mutex::new(None)),
        active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        hotkey_reloader: Arc::new(Mutex::new(None)),
    });

    let (audio_level_tx, audio_level_rx) = crossbeam_channel::bounded::<f32>(128);

    {
        let audio_cfg = cfg_data.audio.clone();
        let recorder = voxctr_audio::AudioRecorder::new(
            audio_cfg,
            app_state.recording.clone(),
            app_state.monitoring.clone(),
            app_state.dynamic_stream.clone(),
            app_state.input_device_index.clone(),
            app_state.gain.clone(),
        );
        let _ = recorder.run(audio_tx, Some(audio_level_tx), Some(app_state.audio_ready.clone()));
    }

    // Spawn a coordinator thread to accumulate audio chunks and trigger batch inference
    let state_for_audio = app_state.clone();
    std::thread::spawn(move || {
        let mut accumulated_audio = Vec::<f32>::new();
        let mut was_recording = false;
        let mut target_id = "default".to_string();

        while let Ok(chunk) = audio_rx.recv() {
            let is_recording = state_for_audio.is_recording();

            if is_recording {
                if !was_recording {
                    accumulated_audio.clear();
                    target_id = state_for_audio.active_target.blocking_lock().clone();
                    was_recording = true;
                }
                accumulated_audio.extend(chunk);
            } else {
                if was_recording {
                    if !accumulated_audio.is_empty() {
                        let req = voxctr_inference::InferenceRequest {
                            audio: std::mem::take(&mut accumulated_audio),
                            target_id: target_id.clone(),
                            context_text: None,
                        };
                        state_for_audio.set_processing(true);
                        let _ = inference_tx.send(req);
                    }
                    was_recording = false;
                }
            }
        }
    });

    // Inference worker
    voxctr_inference::run_worker(cfg_data.clone(), inference_rx, text_tx.clone());

    // ── TTS ───────────────────────────────────────────────────────────────────
    let _tts_handle = if cfg_data.tts.enabled {
        Some(voxctr_tts::TtsEngineWorker::start(cfg_data.tts.clone()))
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

    // ── Hotkey listener ───────────────────────────────────────────────────────
    let (gesture_tx, mut gesture_rx) = voxctr_hotkeys::channel();
    let listener = voxctr_hotkeys::start_listener(
        bindings,
        gesture_tx,
        cfg_data.audio.evdev_device.clone(),
    );

    let state_for_gesture = app_state.clone();
    tokio::spawn(async move {
        let mut reloader = state_for_gesture.hotkey_reloader.lock().await;
        *reloader = Some(listener.reloader_tx);
    });

    let state_for_gesture = app_state.clone();
    tokio::spawn(async move {
        while let Some(event) = gesture_rx.recv().await {
            use voxctr_hotkeys::GestureKind;
            match event.kind {
                GestureKind::Start => {
                    *state_for_gesture.active_target.lock().await = event.target_id.clone();
                    *state_for_gesture.active_binding_label.lock().await = event.binding_label.clone();
                    state_for_gesture.set_recording(true);
                }
                GestureKind::Stop => {
                    state_for_gesture.set_recording(false);
                }
            }
        }
    });

    // ── Text delivery: inference → router → injection ─────────────────────────
    {
        let state = app_state.clone();
        let rt_handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            while let Ok(output) = text_rx.recv() {
                state.set_processing(false);
                if output.text.trim().is_empty() {
                    continue;
                }
                tracing::info!("Received transcription: \"{}\" for target '{}' (took {}ms)", output.text, output.target_id, output.inference_ms);
                let words = output.text.split_whitespace().count() as u32;
                state.increment_words(words);

                // Deliver text via the output target router
                let text = output.text.clone();
                let target_id = output.target_id.clone();
                let router = state.router.clone();
                let state_lt = state.clone();
                let text_lt = output.text.clone();
                rt_handle.spawn(async move {
                    let target_ids: Vec<String> = target_id
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let mut join_set = tokio::task::JoinSet::new();
                    for tid in target_ids {
                        let r = router.clone();
                        let text_clone = text.clone();
                        join_set.spawn(async move {
                            r.deliver(&tid, &text_clone).await;
                        });
                    }
                    // Wait for all deliveries to complete concurrently
                    while let Some(_) = join_set.join_next().await {}
                    *state_lt.last_text.lock().await = text_lt;
                });

                let show_notif = {
                    let cfg_lock = state.config.blocking_lock();
                    cfg_lock.data.ui.show_notification
                };
                if show_notif {
                    voxctr_inject::show_notification("VoxCtr", &output.text);
                }

                // Push to history only when the feature is enabled
                let history_enabled = {
                    let cfg_lock = state.config.blocking_lock();
                    cfg_lock.data.ui.history_enabled
                };
                if history_enabled {
                    let state2 = state.clone();
                    let entry = HistoryEntry {
                        text: output.text,
                        target_id: output.target_id,
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        inference_ms: output.inference_ms,
                    };
                    rt_handle.spawn(async move {
                        let mut hist = state2.history.lock().await;
                        hist.insert(0, entry);
                        if hist.len() > 500 {
                            hist.truncate(500);
                        }
                    });
                }
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
                    v = start_rx.recv() => {
                        if v.is_some() {
                            app_state_dbus.set_recording(true);
                        } else {
                            break;
                        }
                    }
                    v = stop_rx.recv() => {
                        if v.is_some() {
                            app_state_dbus.set_recording(false);
                        } else {
                            break;
                        }
                    }
                }
            }
        });
        tokio::spawn(async move {
            if let Err(e) = voxctr_dbus::start_service(dbus_state, start_tx, stop_tx).await {
                error!("DBus service error: {e}");
            }
        });
    }

    // ── MCP Server ────────────────────────────────────────────────────────────
    if cfg_data.mcp.server_enabled {
        let callbacks = app_state.clone();
        tokio::spawn(async move {
            tracing::info!("Starting MCP server...");
            if let Err(e) = voxctr_mcp::run_server(callbacks).await {
                tracing::error!("MCP server error: {:?}", e);
            }
        });
    }

    // ── Build Tauri app ───────────────────────────────────────────────────────
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, cwd| {
            tracing::info!("Single instance trigger: argv={:?}, cwd={:?}", argv, cwd);
            if let Some(window) = app.get_webview_window("settings") {
                show_and_focus_window(&window);
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(app_state.clone())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label();
                if label == "settings" || label == "history" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .setup(move |app| {
            // ── Forward audio levels to settings window ───────────────────────
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                while let Ok(level) = audio_level_rx.recv() {
                    let _ = handle.emit("audio-level", level);
                }
            });

            // ── System tray ───────────────────────────────────────────────────
            let record_on_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_on.png"))
                .expect("Failed to load record_on icon");
            let record_off_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
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
            let tray_icon = record_off_icon.clone();

            let settings_i = tauri::menu::MenuItem::with_id(app, "settings", "⚙  Settings", true, None::<&str>)?;
            let history_i = tauri::menu::MenuItem::with_id(app, "history", "📋  History", true, None::<&str>)?;
            let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Quit VoxCtr", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(
                app,
                &[&settings_i, &history_i, &separator, &quit_i],
            )?;

            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .tooltip("VoxCtr")
                .menu(&menu)
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "settings" => {
                            if let Some(window) = app.get_webview_window("settings") {
                                show_and_focus_window(&window);
                            }
                        }
                        "history" => {
                            if let Some(window) = app.get_webview_window("history") {
                                show_and_focus_window(&window);
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("settings") {
                            show_and_focus_window(&window);
                        }
                    }
                })
                .build(app)?;

            // Note: Windows ("overlay", "settings", "history") are automatically created by Tauri
            // via the declarations in `tauri.conf.json`. Manual creation here is omitted to prevent duplicates.

            // Automatically show the settings window on startup if configured to do so
            if cfg_data.ui.auto_show_settings {
                if let Some(window) = app.get_webview_window("settings") {
                    show_and_focus_window(&window);
                }
            }

            // Emit periodic status updates to all windows
            let state_for_ticker = app_state.clone();
            let handle = app.handle().clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_millis(150)); // Faster 150ms interval for smooth spin!
                let mut last_recording = false;
                let mut was_animating = false;
                let mut frame_idx = 0;
                loop {
                    interval.tick().await;
                    let is_recording = state_for_ticker.is_recording();
                    let is_processing = state_for_ticker.is_processing();

                    if let Some(tray) = handle.tray_by_id("main-tray") {
                        if is_processing {
                            let icon = &processing_frames[frame_idx];
                            let _ = tray.set_icon(Some(icon.clone()));
                            frame_idx = (frame_idx + 1) % 6;
                            was_animating = true;
                        } else if was_animating || is_recording != last_recording {
                            let icon = if is_recording { &record_on_icon } else { &record_off_icon };
                            let _ = tray.set_icon(Some(icon.clone()));
                            was_animating = false;
                            last_recording = is_recording;
                        }
                    }

                    // Toggle dynamic overlay window visibility based on show_overlay configuration
                    let should_show_overlay = (is_recording || is_processing) && {
                        let cfg = state_for_ticker.config.lock().await;
                        cfg.data.ui.show_overlay
                    };

                    if let Some(window) = handle.get_webview_window("overlay") {
                        if should_show_overlay {
                            let _ = window.show();
                            let _ = window.set_always_on_top(true);
                        } else {
                            let _ = window.hide();
                        }
                    }
                    last_recording = is_recording;

                    let active_target_id = state_for_ticker.active_target.lock().await.clone();
                    let binding_label = state_for_ticker.active_binding_label.lock().await.clone();
                    let target_label = if (is_recording || is_processing) && !binding_label.is_empty() {
                        binding_label
                    } else {
                        let targets_guard = state_for_ticker.targets.lock().await;
                        let ids: Vec<&str> = active_target_id
                            .split(',')
                            .map(|s| s.trim())
                            .filter(|s| !s.is_empty())
                            .collect();
                        let labels: Vec<String> = ids
                            .iter()
                            .map(|id| {
                                targets_guard
                                    .iter()
                                    .find(|t| &t.id == id)
                                    .map(|t| t.label.clone())
                                    .unwrap_or_else(|| {
                                        if *id == "default" {
                                            "Focused Window".to_string()
                                        } else {
                                            id.to_string()
                                        }
                                    })
                            })
                            .collect();
                        labels.join(" + ")
                    };

                    let payload = serde_json::json!({
                        "recording": is_recording,
                        "processing": is_processing,
                        "speaking": state_for_ticker.is_speaking(),
                        "audio_ready": state_for_ticker.is_audio_ready(),
                        "word_count": state_for_ticker.total_words(),
                        "active_target_id": active_target_id,
                        "active_target_label": target_label,
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
            start_monitoring_audio,
            stop_monitoring_audio,
            check_voice_downloaded,
            download_voice,
            check_model_downloaded,
            download_model,
            test_ollama,
        ])
        .run(tauri::generate_context!())
        .expect("error running Tauri application");
}

impl McpCallbacks for AppState {
    fn transcribe_voice(&self, timeout_secs: f64) -> impl std::future::Future<Output = anyhow::Result<String>> + Send {
        async move {
            use std::sync::atomic::Ordering;
            use tokio::time::{sleep, Duration};

            // 1. Clear last_text
            {
                let mut lt = self.last_text.lock().await;
                lt.clear();
            }

            // 2. Start recording
            self.set_recording(true);

            // 3. Spawn a timer task to automatically stop recording after timeout_secs
            let recording = self.recording.clone();
            let audio_tx = self.audio_tx.clone();
            tokio::spawn(async move {
                sleep(Duration::from_secs_f64(timeout_secs)).await;
                recording.store(false, Ordering::SeqCst);
                let _ = audio_tx.send(Vec::new());
            });

            // 4. Wait until recording is set to false (either by the timer or manually/VAD if that's implemented)
            while self.is_recording() {
                sleep(Duration::from_millis(50)).await;
            }

            // 5. Wait a short time for inference to finish and populate last_text
            let poll_limit = 60; // 60 * 50ms = 3.0 seconds maximum wait for inference
            let mut text = String::new();
            for _ in 0..poll_limit {
                sleep(Duration::from_millis(50)).await;
                let current = self.last_text.lock().await.clone();
                if !current.is_empty() {
                    text = current;
                    break;
                }
            }

            if text.is_empty() {
                Ok("(no speech detected)".to_string())
            } else {
                Ok(text)
            }
        }
    }

    fn speak_text(&self, text: String) -> impl std::future::Future<Output = anyhow::Result<()>> + Send {
        async move {
            let handle = self.tts_handle.lock().await;
            if let Some(ref tts) = *handle {
                tts.speak(text);
            }
            Ok(())
        }
    }

    fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send {
        async move {
            (self.is_recording(), self.is_speaking())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicU32};
    use tokio::sync::Mutex;
    use voxctr_config::Config;
    use voxctr_routing::OutputTargetRouter;
    use crate::state::AppState;

    #[tokio::test]
    async fn test_app_state_initial_values() {
        let config = Config::load();
        let config = Arc::new(Mutex::new(config));
        let router = Arc::new(OutputTargetRouter::new(Vec::new()));
        let (audio_tx, _) = crossbeam_channel::bounded(1);

        let state = AppState {
            config,
            router,
            recording: Arc::new(AtomicBool::new(false)),
            processing: Arc::new(AtomicBool::new(false)),
            speaking: Arc::new(AtomicBool::new(false)),
            audio_ready: Arc::new(AtomicBool::new(false)),
            dynamic_stream: Arc::new(AtomicBool::new(false)),
            monitoring: Arc::new(AtomicBool::new(false)),
            input_device_index: Arc::new(AtomicU32::new(u32::MAX)),
            gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            word_count: Arc::new(AtomicU32::new(0)),
            last_text: Arc::new(Mutex::new(String::new())),
            active_target: Arc::new(Mutex::new("default".to_string())),
            active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
            targets: Arc::new(Mutex::new(Vec::new())),
            history: Arc::new(Mutex::new(Vec::new())),
            audio_tx,
            tts_handle: Arc::new(Mutex::new(None)),
            active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        };

        assert!(!state.is_recording());
        assert!(!state.is_speaking());
        assert_eq!(state.total_words(), 0);
        assert_eq!(*state.active_target.lock().await, "default");
    }

    #[tokio::test]
    async fn test_app_state_words_increment() {
        let config = Config::load();
        let config = Arc::new(Mutex::new(config));
        let router = Arc::new(OutputTargetRouter::new(Vec::new()));
        let (audio_tx, _) = crossbeam_channel::bounded(1);

        let state = AppState {
            config,
            router,
            recording: Arc::new(AtomicBool::new(false)),
            processing: Arc::new(AtomicBool::new(false)),
            speaking: Arc::new(AtomicBool::new(false)),
            audio_ready: Arc::new(AtomicBool::new(false)),
            dynamic_stream: Arc::new(AtomicBool::new(false)),
            monitoring: Arc::new(AtomicBool::new(false)),
            input_device_index: Arc::new(AtomicU32::new(u32::MAX)),
            gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            word_count: Arc::new(AtomicU32::new(0)),
            last_text: Arc::new(Mutex::new(String::new())),
            active_target: Arc::new(Mutex::new("default".to_string())),
            active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
            targets: Arc::new(Mutex::new(Vec::new())),
            history: Arc::new(Mutex::new(Vec::new())),
            audio_tx,
            tts_handle: Arc::new(Mutex::new(None)),
            active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        };

        state.increment_words(15);
        assert_eq!(state.total_words(), 15);
        state.increment_words(10);
        assert_eq!(state.total_words(), 25);
    }

    #[tokio::test]
    async fn test_history_entries() {
        let config = Config::load();
        let config = Arc::new(Mutex::new(config));
        let router = Arc::new(OutputTargetRouter::new(Vec::new()));
        let (audio_tx, _) = crossbeam_channel::bounded(1);

        let state = AppState {
            config,
            router,
            recording: Arc::new(AtomicBool::new(false)),
            processing: Arc::new(AtomicBool::new(false)),
            speaking: Arc::new(AtomicBool::new(false)),
            audio_ready: Arc::new(AtomicBool::new(false)),
            dynamic_stream: Arc::new(AtomicBool::new(false)),
            monitoring: Arc::new(AtomicBool::new(false)),
            input_device_index: Arc::new(AtomicU32::new(u32::MAX)),
            gain: Arc::new(AtomicU32::new(1.0f32.to_bits())),
            word_count: Arc::new(AtomicU32::new(0)),
            last_text: Arc::new(Mutex::new(String::new())),
            active_target: Arc::new(Mutex::new("default".to_string())),
            active_binding_label: Arc::new(Mutex::new("Focused Window".to_string())),
            targets: Arc::new(Mutex::new(Vec::new())),
            history: Arc::new(Mutex::new(Vec::new())),
            audio_tx,
            tts_handle: Arc::new(Mutex::new(None)),
            active_fifos: Arc::new(Mutex::new(std::collections::HashSet::new())),
        };

        {
            let mut hist = state.history.lock().await;
            hist.push(HistoryEntry {
                text: "hello world".to_string(),
                target_id: "default".to_string(),
                timestamp: "2026-05-20T22:00:00Z".to_string(),
                inference_ms: 120,
            });
        }

        let hist = state.history.lock().await;
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].text, "hello world");
        assert_eq!(hist[0].target_id, "default");
        assert_eq!(hist[0].inference_ms, 120);
    }

    #[tokio::test]
    async fn test_concurrent_multi_target_delivery() {
        use voxctr_routing::models::{DeliveryType, OutputTarget};
        let target_a = OutputTarget {
            id: "target_a".into(),
            label: "Target A".into(),
            delivery: DeliveryType::Clipboard,
            command: None,
            pipe_path: None,
            socket_host: None,
            socket_port: None,
            socket_unix: None,
            file_path: None,
            file_prefix: "".into(),
            file_timestamp: false,
            file_mode: "append".into(),
            dbus_signal: None,
            http_url: None,
            http_method: "POST".into(),
            http_headers: None,
            http_json_template: None,
            webhook_url: None,
            webhook_secret: None,
            webhook_json_template: None,
            mcp_path: None,
            mcp_tool: None,
            mcp_args: None,
            send_on_release: true,
            append_newline: false,
            initial_prompt: None,
            processing: Default::default(),
            response_pipe: None,
            tts_engine: "piper".into(),
            tts_voice: None,
        };

        let target_b = OutputTarget {
            id: "target_b".into(),
            label: "Target B".into(),
            delivery: DeliveryType::Clipboard,
            command: None,
            pipe_path: None,
            socket_host: None,
            socket_port: None,
            socket_unix: None,
            file_path: None,
            file_prefix: "".into(),
            file_timestamp: false,
            file_mode: "append".into(),
            dbus_signal: None,
            http_url: None,
            http_method: "POST".into(),
            http_headers: None,
            http_json_template: None,
            webhook_url: None,
            webhook_secret: None,
            webhook_json_template: None,
            mcp_path: None,
            mcp_tool: None,
            mcp_args: None,
            send_on_release: true,
            append_newline: false,
            initial_prompt: None,
            processing: Default::default(),
            response_pipe: None,
            tts_engine: "piper".into(),
            tts_voice: None,
        };

        let targets = vec![target_a, target_b];

        let router = Arc::new(OutputTargetRouter::new(targets));
        let text = "Concurrent delivery text".to_string();
        let target_ids = vec!["target_a".to_string(), "target_b".to_string()];

        let mut join_set = tokio::task::JoinSet::new();
        for tid in target_ids {
            let r = router.clone();
            let text_clone = text.clone();
            join_set.spawn(async move {
                r.deliver(&tid, &text_clone).await
            });
        }

        let mut results = Vec::new();
        while let Some(res) = join_set.join_next().await {
            results.push(res.unwrap());
        }

        assert_eq!(results.len(), 2);
        for res in results {
            assert!(res.success);
        }
    }
}

