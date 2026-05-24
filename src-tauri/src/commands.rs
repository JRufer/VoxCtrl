use std::sync::Arc;

use tauri::{Emitter, Manager, State};
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
        processing: state.is_processing(),
        speaking: state.is_speaking(),
        audio_ready: state.is_audio_ready(),
        word_count: state.total_words(),
        active_target_id,
        active_target_label: target_label,
    })
}

#[derive(serde::Serialize)]
pub struct StatusPayload {
    pub recording: bool,
    pub processing: bool,
    pub speaking: bool,
    pub audio_ready: bool,
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
    app: tauri::AppHandle,
    new_config: AppConfig,
) -> Result<(), String> {
    // Update live dynamic stream state, input device index, and gain in AppState
    state.set_dynamic_stream(new_config.audio.dynamic_stream);
    state.set_input_device_index(new_config.audio.input_device_index);
    state.set_gain(new_config.audio.gain);

    let mut guard = state.config.lock().await;
    guard.data = new_config.clone();
    guard.save().map_err(|e| e.to_string())?;
    info!("Config saved");

    // Emit config-changed event to all windows to enable instant reactivity
    let _ = app.emit("config-changed", new_config);

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
    state.router.reload(targets).await;
    info!("Targets saved and router reloaded");

    // Dynamically spawn new FIFO response pipe listeners if TTS is active
    let tts_handle_opt = {
        let guard = state.tts_handle.lock().await;
        guard.clone()
    };
    if let Some(tts) = tts_handle_opt {
        state.spawn_fifo_responders(tts).await;
    }

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
    state: State<'_, Arc<AppState>>,
    bindings: Vec<HotkeyBinding>,
) -> Result<(), String> {
    let dir = voxctr_routing::config_dir();
    voxctr_routing::save_bindings(&bindings, &dir).map_err(|e| e.to_string())?;
    info!("Bindings saved");
    
    // Hot reload the bindings in the active listener threads
    let reloader_guard = state.hotkey_reloader.lock().await;
    if let Some(reloader) = &*reloader_guard {
        if let Err(e) = reloader.send(bindings) {
            tracing::warn!("Failed to hot-reload bindings: {e}");
        } else {
            info!("Hot-reload signal sent to listener");
        }
    }
    
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

#[tauri::command]
pub async fn speak_text(
    state: State<'_, Arc<AppState>>,
    text: String,
    voice: Option<String>,
) -> Result<(), String> {
    info!("TTS speak_text via command: {text}");
    let handle = state.tts_handle.lock().await;
    if let Some(ref tts) = *handle {
        tts.speak_utterance(voxctr_tts::Utterance {
            text,
            voice,
            source_label: None,
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn check_voice_downloaded(voice_name: String) -> Result<bool, String> {
    Ok(voxctr_tts::is_voice_downloaded(&voice_name))
}

#[tauri::command]
pub async fn download_voice(voice_name: String) -> Result<(), String> {
    voxctr_tts::download_voice(&voice_name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_model_downloaded(model_size: String) -> Result<bool, String> {
    Ok(voxctr_inference::whisper_cpp::is_model_downloaded(&model_size))
}

#[tauri::command]
pub async fn download_model(model_size: String) -> Result<(), String> {
    voxctr_inference::whisper_cpp::download_model(&model_size)
        .await
        .map_err(|e| e.to_string())
}

// ── Overlay window ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn show_overlay(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("overlay") {
        w.show().map_err(|e| e.to_string())?;
        w.set_always_on_top(true).map_err(|e| e.to_string())?;
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
pub async fn start_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(true);
    info!("Live audio monitoring started");
    Ok(())
}

#[tauri::command]
pub async fn stop_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(false);
    info!("Live audio monitoring stopped");
    Ok(())
}

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

#[derive(serde::Serialize)]
pub struct OllamaTestResult {
    pub success: bool,
    pub message: String,
    pub models: Vec<String>,
}

#[tauri::command]
pub async fn test_ollama(
    endpoint: String,
    timeout_secs: u64,
) -> Result<OllamaTestResult, String> {
    use voxctr_config::{OllamaConfig, OllamaMode};
    let cfg = OllamaConfig {
        enabled: true,
        endpoint: endpoint.clone(),
        model: String::new(),
        mode: OllamaMode::Clean,
        custom_prompt: None,
        timeout_secs,
    };
    let client = voxctr_llm::OllamaClient::new(cfg);
    if client.is_available().await {
        match client.list_models().await {
            Ok(models) => {
                Ok(OllamaTestResult {
                    success: true,
                    message: "Successfully connected to Ollama!".to_string(),
                    models,
                })
            }
            Err(e) => {
                Ok(OllamaTestResult {
                    success: true,
                    message: format!("Successfully connected, but failed to fetch model list: {}", e),
                    models: Vec::new(),
                })
            }
        }
    } else {
        Ok(OllamaTestResult {
            success: false,
            message: format!("Failed to connect to Ollama at '{}'. Make sure Ollama is running.", endpoint),
            models: Vec::new(),
        })
    }
}

