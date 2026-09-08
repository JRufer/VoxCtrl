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

pub fn create_tray(app: &tauri::App) -> Result<tauri::tray::TrayIcon, tauri::Error> {
    let record_off_icon = tauri::image::Image::from_bytes(include_bytes!("../../assets/record_off.png"))
        .expect("Failed to load record_off icon");
    let tray_icon = record_off_icon.clone();

    let settings_i = tauri::menu::MenuItem::with_id(app, "settings", "⚙  Settings", true, None::<&str>)?;
    let setup_i = tauri::menu::MenuItem::with_id(app, "setup", TRAY_SETUP_OK, true, None::<&str>)?;
    let separator = tauri::menu::PredefinedMenuItem::separator(app)?;
    let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "Quit VoxCtrl", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(
        app,
        &[&settings_i, &setup_i, &separator, &quit_i],
    )?;
    let _ = SETUP_MENU_ITEM.set(setup_i);

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
                "quit" => {
                    app.exit(0);
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

            // Forward status to Slint overlay channel. When the overlay is
            // disabled, force the visibility flags off so the native window
            // never maps — a mapped overlay grabs keyboard focus on Wayland
            // and prevents transcribed text from reaching the cursor.
            let overlay_on = state_for_ticker.is_overlay_enabled();
            let mut payload_value = payload.clone();
            if let Some(obj) = payload_value.as_object_mut() {
                obj.insert("type".to_string(), serde_json::json!("status"));
                obj.insert("audio_level".to_string(), serde_json::json!(0.0));
                let overlay_style = last_pos.as_ref().map(|(_, _, style)| style.as_str());
                obj.insert("overlay_style".to_string(), serde_json::json!(overlay_style));
                if !overlay_on {
                    obj.insert("recording".to_string(), serde_json::json!(false));
                    obj.insert("processing".to_string(), serde_json::json!(false));
                    obj.insert("speaking".to_string(), serde_json::json!(false));
                }
            }
            if let Ok(json_str) = serde_json::to_string(&payload_value) {
                let _ = state_for_ticker.overlay_tx.send(json_str);
            }
        }
    });
}
