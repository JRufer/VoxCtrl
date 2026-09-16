use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter,
};
use crate::state::AppState;

/// The tray entry that doubles as the setup indicator.
static SETUP_MENU_ITEM: OnceLock<tauri::menu::MenuItem<tauri::Wry>> = OnceLock::new();

pub const TRAY_SETUP_OK: &str = "🩺  Setup & Diagnostics";
#[cfg(target_os = "linux")]
pub const TRAY_SETUP_BROKEN: &str = "⚠️  Global shortcuts unavailable";

/// Reflect setup state in the tray, which is the one piece of VoxCtrl UI that
/// is always on screen.
#[cfg(target_os = "linux")]
pub fn update_tray_for_setup(app: &tauri::AppHandle, ok: bool) {
    let app = app.clone();
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(item) = SETUP_MENU_ITEM.get() {
            let _ = item.set_text(if ok { TRAY_SETUP_OK } else { TRAY_SETUP_BROKEN });
        }
        if let Some(tray) = handle.tray_by_id("main-tray") {
            let _ = tray.set_tooltip(Some(if ok {
                "VoxCtrl"
            } else {
                "VoxCtrl — global shortcuts are unavailable"
            }));
        }
    });
}

/// The tray entry that mirrors (and toggles) the TTS memory mode.
static TTS_MEMORY_MENU_ITEM: OnceLock<tauri::menu::CheckMenuItem<tauri::Wry>> = OnceLock::new();

pub const TRAY_TTS_MEMORY: &str = "🧠  Unload TTS model when idle";

/// Keep the tray checkbox in step with the setting, whichever side changed it
/// (the tray item itself, or the TTS settings tab).
pub fn update_tray_tts_memory(app: &tauri::AppHandle, on_demand: bool) {
    let app = app.clone();
    let _ = app.run_on_main_thread(move || {
        if let Some(item) = TTS_MEMORY_MENU_ITEM.get() {
            let _ = item.set_checked(on_demand);
        }
    });
}

/// Flip the TTS memory mode from the tray: persist it, tell the running worker
/// (no restart — that would tear down the engine and its audio device), and
/// mirror the new state back into the settings window.
fn toggle_tts_memory_mode(app: &tauri::AppHandle) {
    use tauri::Manager;

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<Arc<AppState>>().inner().clone();

        let new_data = {
            let mut guard = state.config.lock().await;
            let on_demand = !guard.data.tts.unloads_when_idle();
            guard.data.tts.memory_mode = if on_demand {
                voxctrl_config::TtsMemoryMode::OnDemand
            } else {
                voxctrl_config::TtsMemoryMode::AlwaysLoaded
            };
            if let Err(e) = guard.save() {
                tracing::error!("Failed to save TTS memory mode from tray: {e}");
            }
            guard.data.clone()
        };

        let on_demand = new_data.tts.unloads_when_idle();
        if let Some(tts) = state.tts_handle.lock().await.as_ref() {
            tts.update_config(new_data.tts.clone());
        }

        update_tray_tts_memory(&app, on_demand);
        let _ = app.emit("config-changed", new_data.clone());

        voxctrl_inject::show_notification(
            "VoxCtrl",
            &if on_demand {
                format!(
                    "TTS model will unload after {} minutes idle to save memory.",
                    new_data.tts.idle_unload_duration().as_secs() / 60
                )
            } else {
                "TTS model will stay loaded in memory for the fastest response.".to_string()
            },
        );
    });
}

pub fn create_tray(app: &tauri::App) -> Result<tauri::tray::TrayIcon, tauri::Error> {
    let record_off_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
        .expect("Failed to load record_off icon");
    let tray_icon = record_off_icon.clone();

    let settings_i = tauri::menu::MenuItem::with_id(app, "settings", "⚙  Settings", true, None::<&str>)?;
    let setup_i = tauri::menu::MenuItem::with_id(app, "setup", TRAY_SETUP_OK, true, None::<&str>)?;
    let tts_memory_i = tauri::menu::CheckMenuItem::with_id(
        app,
        "tts_memory",
        TRAY_TTS_MEMORY,
        true,
        // The real value is applied right after the tray is built (see
        // `sync_tts_memory_item`); a `try_lock` here can miss at startup.
        false,
        None::<&str>,
    )?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Quit VoxCtrl", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(
        app,
        &[&settings_i, &setup_i, &tts_memory_i, &separator, &quit_i],
    )?;
    let _ = SETUP_MENU_ITEM.set(setup_i);
    let _ = TTS_MEMORY_MENU_ITEM.set(tts_memory_i);

    TrayIconBuilder::with_id("main-tray")
        .icon(tray_icon)
        .tooltip("VoxCtrl")
        .menu(&menu)
        .on_menu_event(|app, event| {
            match event.id().as_ref() {
                "settings" => {
                    if let Err(e) = crate::window::open_settings_window(app) {
                        tracing::error!("Could not open Settings: {e}");
                    }
                }
                "setup" => {
                    crate::window::show_setup_window();
                }
                "tts_memory" => {
                    toggle_tts_memory_mode(app);
                }
                "quit" => {
                    // Best-effort: lets the TTS worker stop playback and shut
                    // down its resident audio.cpp session promptly rather
                    // than leaving that to `PR_SET_PDEATHSIG` alone (which
                    // still catches it if this doesn't finish in time).
                    use tauri::Manager;
                    let app = app.clone();
                    tauri::async_runtime::spawn(async move {
                        let state = app.state::<Arc<AppState>>().inner().clone();
                        if let Some(tts) = state.tts_handle.lock().await.as_ref() {
                            tts.shutdown();
                        }
                        app.exit(0);
                    });
                }
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                if let Err(e) = crate::window::open_settings_window(tray.app_handle()) {
                    tracing::error!("Could not open Settings: {e}");
                }
            }
        })
        .build(app)
}

/// Show the stored TTS memory mode on the freshly built tray item.
pub fn sync_tts_memory_item(app: tauri::AppHandle, state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let on_demand = state.config.lock().await.data.tts.unloads_when_idle();
        update_tray_tts_memory(&app, on_demand);
    });
}

/// How long the overlay window stays alive after nothing is left to show,
/// before it's actually destroyed. Long enough for the frontend's own
/// outro fade (`Overlay.svelte`'s 450ms unmount timeout) to finish playing
/// inside a still-live window; a fast reactivation within this window finds
/// the overlay already there and never triggers a destroy/recreate cycle
/// at all.
const OVERLAY_HIDE_DEBOUNCE: Duration = Duration::from_millis(500);

/// Emits `status-tick`, animates the tray icon, and owns the dictation
/// overlay window's entire lifecycle.
///
/// The overlay is created fresh (`window::open_overlay_window`) the moment
/// there's something to show, and destroyed (`window::hide_overlay`) again
/// once idle for `OVERLAY_HIDE_DEBOUNCE`, rather than being created once at
/// startup and left mapped for the app's whole session the way it used to
/// be. That's a direct consequence of a long debugging history: on some
/// systems (confirmed: KDE, XWayland) WebKitGTK never repaints this
/// window's buffer back to blank on its own, no matter how a repaint is
/// requested or how tightly the request is ordered against a reactivation —
/// every variant of "hide it and trust a forced repaint lands before it's
/// shown again" left a stale frame reappearing the instant the window was
/// un-hidden. Destroying the window sidesteps the question entirely: a
/// freshly created `WebviewWindow` has never had anything painted into it.
///
/// This has to live here rather than in the overlay's own frontend
/// (`Overlay.svelte`), which is where it lived first and is a dead end:
/// destroying the window that's running the code deciding when to bring it
/// back also destroys that code. The very first idle cycle after launch —
/// which happens automatically, since nothing is recording yet — would
/// destroy the window and, with it, the only thing that could ever have
/// asked for it to come back. Nothing else was left watching for the next
/// activation. This ticker already polls every piece of state that decides
/// overlay visibility (recording, speaking, MCP recording) for the tray
/// icon and `status-tick`, and it's a plain sequential loop, so it can own
/// the window's lifecycle too without needing any locking of its own: it is
/// the only thing that ever touches the overlay window, by construction.
pub fn spawn_status_ticker(
    handle: tauri::AppHandle,
    state_for_ticker: Arc<AppState>,
    record_on_icon: tauri::image::Image<'static>,
    record_off_icon: tauri::image::Image<'static>,
    processing_frames: [tauri::image::Image<'static>; 6],
) {
    tokio::spawn(async move {
        // The tray animation frame rate, and the ceiling on how quickly a
        // state change reaches the UI.
        let mut interval = tokio::time::interval(Duration::from_millis(150));
        // The status window falls back to polling when ticks stop arriving,
        // so an idle app still sends one of these — just not seven a second.
        const HEARTBEAT: Duration = Duration::from_millis(900);

        let mut last_recording = false;
        let mut was_animating = false;
        let mut frame_idx = 0;
        let mut last_pos: Option<(String, String, String)> = None;
        let mut startup_tick_count: u32 = 0;
        // Whether the overlay window currently exists, and (while it
        // shouldn't) how long it's been idle — see this function's doc
        // comment for why this loop owns creating/destroying it.
        let mut overlay_visible = false;
        let mut overlay_idle_since: Option<tokio::time::Instant> = None;
        // What the last emitted payload said, so a tick that changes nothing
        // costs a few atomic loads instead of building a payload, two JSON
        // encodes, a webview event and a message to the overlay process.
        let mut last_flags: Option<(bool, bool, bool, bool, bool, u32)> = None;
        let mut last_emit = tokio::time::Instant::now() - HEARTBEAT;
        // The label is derived from three rarely-changing strings; caching it
        // keeps the common tick from rebuilding and re-joining it.
        let mut label_inputs: Option<(String, String, bool)> = None;
        let mut cached_label = String::new();

        loop {
            interval.tick().await;
            startup_tick_count = startup_tick_count.saturating_add(1);
            let is_recording = state_for_ticker.is_recording();
            let is_processing = state_for_ticker.is_processing();

            // Decide whether the tray icon needs updating this tick and,
            // if so, which frame to show. The actual `set_icon` call must
            // happen on the GTK main thread: on Linux the tray is backed by
            // ayatana-appindicator/GTK, which is not thread-safe. Calling it
            // from this Tokio worker thread makes icon updates unreliable —
            // the animated icon flickers or disappears entirely on
            // appindicator-based desktops (e.g. GNOME). `run_on_main_thread`
            // marshals the update onto the loop that owns the tray.
            let next_icon: Option<tauri::image::Image<'static>> = if is_processing {
                let icon = processing_frames[frame_idx].clone();
                frame_idx = (frame_idx + 1) % 6;
                was_animating = true;
                Some(icon)
            } else if was_animating || is_recording != last_recording {
                was_animating = false;
                Some(if is_recording { record_on_icon.clone() } else { record_off_icon.clone() })
            } else {
                None
            };

            if let Some(icon) = next_icon {
                let handle_for_icon = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Some(tray) = handle_for_icon.tray_by_id("main-tray") {
                        let _ = tray.set_icon(Some(icon));
                    }
                });
            }

            last_recording = is_recording;

            // Overlay placement follows the user's config, which is compared
            // under the lock so an unchanged setting costs no allocation.
            let mut position_changed = false;
            {
                let cfg = state_for_ticker.config.lock().await;
                let ui = &cfg.data.ui;
                let unchanged = last_pos.as_ref().is_some_and(|(pos, mon, style)| {
                    pos == &ui.overlay_position && mon == &ui.overlay_monitor && style == &ui.overlay_style
                });
                if !unchanged {
                    last_pos = Some((
                        ui.overlay_position.clone(),
                        ui.overlay_monitor.clone(),
                        ui.overlay_style.clone(),
                    ));
                    position_changed = true;
                }
            }
            if position_changed || startup_tick_count < 40 {
                // Send the anchor + monitor; the overlay computes pixel
                // coordinates itself using its own display scale.
                let (position, monitor) = last_pos
                    .as_ref()
                    .map(|(pos, mon, _)| (pos.as_str(), mon.as_str()))
                    .expect("set above");
                let pos_msg = serde_json::json!({
                    "type": "position",
                    "position": position,
                    "monitor": monitor,
                });
                if let Ok(json_str) = serde_json::to_string(&pos_msg) {
                    let _ = state_for_ticker.overlay_tx.send(json_str);
                }
            }

            // Decide whether the overlay window should exist right now, and
            // create/destroy it to match — see this function's doc comment
            // for why that lifecycle lives here. Mirrors the same condition
            // Overlay.svelte derives client-side for its own content
            // (`isRecordingOrSpeaking`), since that logic still decides what
            // to render inside the window once it exists; this only decides
            // whether the window itself exists at all.
            let should_show_overlay = {
                let cfg = state_for_ticker.config.lock().await;
                (is_recording && cfg.data.ui.show_overlay)
                    || (state_for_ticker.is_speaking()
                        && cfg.data.tts.enabled
                        && cfg.data.tts.response_overlay)
                    || (state_for_ticker.is_mcp_recording() && cfg.data.mcp.visual_feedback)
                    || state_for_ticker.is_command_overlay_active()
            };
            if should_show_overlay {
                overlay_idle_since = None;
                if !overlay_visible {
                    let (position, monitor) = last_pos
                        .as_ref()
                        .map(|(pos, mon, _)| (pos.as_str(), mon.as_str()))
                        .unwrap_or(("center", "primary"));
                    match crate::window::open_overlay_window(&handle, position, monitor) {
                        Ok(_) => overlay_visible = true,
                        Err(e) => tracing::error!("Failed to open the dictation overlay: {e}"),
                    }
                }
            } else if overlay_visible {
                match overlay_idle_since {
                    None => overlay_idle_since = Some(tokio::time::Instant::now()),
                    Some(since) if since.elapsed() >= OVERLAY_HIDE_DEBOUNCE => {
                        crate::window::hide_overlay(&handle);
                        overlay_visible = false;
                        overlay_idle_since = None;
                    }
                    _ => {}
                }
            }

            let active_target_id = state_for_ticker.active_target.lock().await.clone();
            let binding_label = state_for_ticker.active_binding_label.lock().await.clone();
            let use_binding_label = (is_recording || is_processing) && !binding_label.is_empty();
            let label_changed = match &label_inputs {
                Some((target, binding, from_binding)) => {
                    target != &active_target_id
                        || binding != &binding_label
                        || *from_binding != use_binding_label
                }
                None => true,
            };
            if label_changed {
                label_inputs = Some((
                    active_target_id.clone(),
                    binding_label.clone(),
                    use_binding_label,
                ));
                cached_label = if use_binding_label {
                    binding_label
                } else {
                    let targets_guard = state_for_ticker.targets.lock().await;
                    active_target_id
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|id| {
                            targets_guard
                                .iter()
                                .find(|t| t.id == id)
                                .map(|t| t.label.clone())
                                .unwrap_or_else(|| {
                                    if id == "default" {
                                        "Focused Window".to_string()
                                    } else {
                                        id.to_string()
                                    }
                                })
                        })
                        .collect::<Vec<_>>()
                        .join(" + ")
                };
            }

            // Everything the payload carries, compared before one is built:
            // the label and target id are the cached strings above, so an
            // unchanged tick allocates nothing at all.
            let flags = (
                is_recording,
                is_processing,
                state_for_ticker.is_speaking(),
                state_for_ticker.is_mcp_recording(),
                state_for_ticker.is_audio_ready(),
                state_for_ticker.total_words(),
            );
            let now = tokio::time::Instant::now();
            let unchanged = last_flags == Some(flags) && !label_changed;
            if unchanged && now.duration_since(last_emit) < HEARTBEAT {
                continue;
            }
            last_flags = Some(flags);
            last_emit = now;

            let payload = serde_json::json!({
                "recording": flags.0,
                "processing": flags.1,
                "speaking": flags.2,
                "mcp_recording": flags.3,
                "audio_ready": flags.4,
                "word_count": flags.5,
                "active_target_id": &active_target_id,
                "active_target_label": &cached_label,
            });

            let _ = handle.emit("status-tick", &payload);
        }
    });
}
