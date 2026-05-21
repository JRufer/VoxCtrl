use std::sync::Arc;

use tauri::{Manager, State};
use tracing::info;
use voxctr_config::AppConfig;
use voxctr_routing::{HotkeyBinding, OutputTarget};

use crate::state::{AppState, HistoryEntry};

// ── Status ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusPayload, String> {
    let active_target_id = state.active_target.lock().await.clone();
    let target_label = {
        let targets_guard = state.targets.lock().await;
        targets_guard.iter()
            .find(|t| t.id == active_target_id)
            .map(|t| t.label.clone())
            .unwrap_or_else(|| {
                if active_target_id == "default" {
                    "Focused Window".to_string()
                } else {
                    active_target_id.clone()
                }
            })
    };

    Ok(StatusPayload {
        recording: state.is_recording(),
        speaking: state.is_speaking(),
        word_count: state.total_words(),
        active_target_id,
        active_target_label: target_label,
    })
}

#[derive(serde::Serialize)]
pub struct StatusPayload {
    pub recording: bool,
    pub speaking: bool,
    pub word_count: u32,
    pub active_target_id: String,
    pub active_target_label: String,
}

// ── Recording control ─────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_recording(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_recording(true);
    info!("Recording started via command");
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_recording(false);
    info!("Recording stopped via command");
    Ok(())
}

#[tauri::command]
pub async fn toggle_recording(state: State<'_, Arc<AppState>>) -> Result<bool, String> {
    let was = state.is_recording();
    state.set_recording(!was);
    Ok(!was)
}

// ── Config ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<AppConfig, String> {
    let guard = state.config.lock().await;
    Ok(guard.data.clone())
}

#[tauri::command]
pub async fn save_config(
    state: State<'_, Arc<AppState>>,
    new_config: AppConfig,
) -> Result<(), String> {
    let mut guard = state.config.lock().await;
    guard.data = new_config;
    guard.save().map_err(|e| e.to_string())?;
    info!("Config saved");
    Ok(())
}

// ── Routing ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_targets(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<OutputTarget>, String> {
    let dir = voxctr_routing::config_dir();
    voxctr_routing::load_targets(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_targets(
    state: State<'_, Arc<AppState>>,
    targets: Vec<OutputTarget>,
) -> Result<(), String> {
    let dir = voxctr_routing::config_dir();
    voxctr_routing::save_targets(&targets, &dir).map_err(|e| e.to_string())?;
    
    // Update the in-memory targets cache
    *state.targets.lock().await = targets.clone();

    // Hot-reload the router
    let router = state.router.lock().await;
    router.reload(targets).await;
    info!("Targets saved and router reloaded");
    Ok(())
}

#[tauri::command]
pub async fn get_bindings(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<HotkeyBinding>, String> {
    let dir = voxctr_routing::config_dir();
    voxctr_routing::load_bindings(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_bindings(
    _state: State<'_, Arc<AppState>>,
    bindings: Vec<HotkeyBinding>,
) -> Result<(), String> {
    let dir = voxctr_routing::config_dir();
    voxctr_routing::save_bindings(&bindings, &dir).map_err(|e| e.to_string())?;
    info!("Bindings saved");
    Ok(())
}

// ── History ───────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn get_history(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<HistoryEntry>, String> {
    let guard = state.history.lock().await;
    Ok(guard.clone())
}

#[tauri::command]
pub async fn clear_history(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.history.lock().await.clear();
    state.word_count.store(0, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

// ── TTS ───────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn speak_text(
    _app: tauri::AppHandle,
    text: String,
) -> Result<(), String> {
    // Retrieve TTS handle from app state extension
    info!("TTS speak_text via command: {text}");
    // Actual TTS invocation wired in lib.rs via app.state()
    Ok(())
}

// ── Overlay window ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn show_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("overlay") {
        w.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub async fn hide_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("overlay") {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

// ── Audio devices ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let devices = voxctr_audio::list_input_devices();
    Ok(devices
        .into_iter()
        .map(|d| AudioDeviceInfo { index: d.index, name: d.name })
        .collect())
}

#[derive(serde::Serialize)]
pub struct AudioDeviceInfo {
    pub index: u32,
    pub name: String,
}
