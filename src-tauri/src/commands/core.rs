//! Status, recording control, config and build-info commands.

use std::sync::Arc;

use tauri::{Emitter, State};
use tracing::info;
use voxctrl_config::AppConfig;

use crate::state::AppState;

#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusPayload, String> {
    let active_target_id = state.active_target.lock().await.clone();
    let target_label =
        voxctrl_routing::targets_display_label(&active_target_id, &state.targets.lock().await);

    Ok(StatusPayload {
        recording: state.is_recording(),
        processing: state.is_processing(),
        speaking: state.is_speaking(),
        mcp_recording: state.is_mcp_recording(),
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
    pub mcp_recording: bool,
    pub audio_ready: bool,
    pub word_count: u32,
    pub active_target_id: String,
    pub active_target_label: String,
}

#[tauri::command]
pub async fn start_recording(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    *state.active_binding_id.lock().await = String::new();
    state.begin_recording().await;
    info!("Recording started via command");
    let active_target = state.active_target.lock().await.clone();
    state.preload_tts_for_target(&active_target).await;
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
    if !was {
        *state.active_binding_id.lock().await = String::new();
        let active_target = state.active_target.lock().await.clone();
        state.preload_tts_for_target(&active_target).await;
    }
    state.set_recording(!was);
    Ok(!was)
}

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
    state.set_noise_suppression(new_config.audio.noise_suppression);
    state.set_overlay_enabled(new_config.ui.show_overlay);

    // Dynamic TTS engine lifecycle management: a running worker takes the new
    // settings live; one is started only when TTS was off until now.
    {
        let mut handle = state.tts_handle.lock().await;
        if let Some(ref tts) = *handle {
            tts.update_config(new_config.tts.clone());
        } else if new_config.tts.enabled {
            *handle = Some(crate::services::start_tts_worker(&app, state.inner(), &new_config));
        }
    }

    let mut guard = state.config.lock().await;
    let stop_key_changed = guard.data.tts.stop_key != new_config.tts.stop_key;
    guard.data = new_config.clone();
    guard.save().map_err(|e| e.to_string())?;
    info!("Config saved");

    // Hot-reload inference engine configuration
    let _ = state.inference_config_tx.send(Arc::new(new_config.clone()));

    let (overlay_position, overlay_monitor) = (
        guard.data.ui.overlay_position.clone(),
        guard.data.ui.overlay_monitor.clone(),
    );

    // Hot-reload the stop key binding in the listener if tts.stop_key changed.
    // The config lock has to go first: assembling the set reads the stop key
    // back out of it.
    drop(guard);
    if stop_key_changed {
        // A set that could not be read is not worth reloading: the listener
        // keeps the shortcuts it already has instead of losing all of them.
        if let Some(bindings) = crate::stop_key::listener_bindings_from_disk(&state).await {
            let reloader_guard = state.hotkey_reloader.lock().await;
            if let Some(reloader) = &*reloader_guard {
                let _ = reloader.send(bindings);
            }
        }
    }

    // Keep the tray checkbox in step with the TTS memory setting.
    crate::tray::update_tray_tts_memory(&app, new_config.tts.unloads_when_idle());

    // Emit config-changed event to all windows to enable instant reactivity
    let _ = app.emit("config-changed", new_config);

    let pos_msg = serde_json::json!({
        "type": "position",
        "position": overlay_position,
        "monitor": overlay_monitor,
    });
    if let Ok(json_str) = serde_json::to_string(&pos_msg) {
        let _ = state.overlay_tx.send(json_str);
    }

    Ok(())
}

#[tauri::command]
pub async fn stop_tts(
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    info!("TTS stop requested via command");
    let handle = state.tts_handle.lock().await;
    if let Some(ref tts) = *handle {
        tts.stop();
    }
    Ok(())
}

/// Returns true when this binary was compiled with the `cuda` cargo feature.
/// The frontend uses this to show or hide the CUDA device option.
#[tauri::command]
pub fn cuda_enabled() -> bool {
    cfg!(feature = "cuda")
}

/// What this build can actually offload to a GPU, per engine.
///
/// Both fields are decided at compile time and neither can be inferred from the
/// other: the Vulkan build gives whisper.cpp the GPU and leaves Moonshine on the
/// CPU, because ONNX Runtime has no Vulkan execution provider. The Engine tab
/// asks for this so it can offer the device options the build can honour, and
/// name the engine that cannot use the GPU at all.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AcceleratorSupport {
    /// `"cuda"`, `"vulkan"`, or `None` on a CPU-only build.
    pub whisper_gpu: Option<String>,
    /// `"cuda"`, `"coreml"`, or `None` — `None` in every build shipped today.
    pub moonshine_gpu: Option<String>,
    /// `"cuda"`, `"coreml"`, `"webgpu"`, or `None`.
    pub parakeet_gpu: Option<String>,
    /// `"vulkan"` or `None`.
    pub s1_mini_gpu: Option<String>,
}

#[tauri::command]
pub fn accelerator_support() -> AcceleratorSupport {
    AcceleratorSupport {
        whisper_gpu: voxctrl_inference::whisper_gpu_backend().map(str::to_string),
        moonshine_gpu: voxctrl_inference::moonshine_gpu_backend().map(str::to_string),
        parakeet_gpu: voxctrl_inference::parakeet_gpu_backend().map(str::to_string),
        s1_mini_gpu: voxctrl_inference::s1_mini_gpu_backend().map(str::to_string),
    }
}
