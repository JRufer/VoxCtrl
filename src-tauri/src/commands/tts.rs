//! Text-to-speech: speaking, voices and TTS model downloads.

use std::sync::Arc;

use tauri::State;
use tracing::info;

use crate::state::AppState;

#[tauri::command]
pub async fn speak_text(
    state: State<'_, Arc<AppState>>,
    text: String,
    voice: Option<String>,
) -> Result<(), String> {
    info!("TTS speak_text via command: {text}");
    let handle = state.tts_handle.lock().await;
    if let Some(ref tts) = *handle {
        tts.speak_utterance(voxctrl_tts::Utterance {
            text,
            voice,
            source_label: None,
        });
    }
    Ok(())
}

#[tauri::command]
pub async fn check_voice_downloaded(voice_name: String, voice_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_voice_downloaded(&voice_name, &voice_dir))
}

#[tauri::command]
pub async fn download_voice(voice_name: String, voice_dir: String) -> Result<(), String> {
    voxctrl_tts::download_voice(&voice_name, &voice_dir)
        .await
        .map_err(|e| e.to_string())
}

/// The HuggingFace token exported into the environment, if any.
///
/// The UI shows it in place of the configured one and stops editing, because
/// `HF_TOKEN` wins at download time — a value typed over it would be saved and
/// then ignored, which is worse than not offering the field.
#[tauri::command]
pub async fn hf_token_env() -> Option<String> {
    voxctrl_tts::hf_token_from_env()
}

#[tauri::command]
pub async fn check_breeze_tts_2_ready(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_breeze_tts_2_ready(&model_dir))
}

#[tauri::command]
pub async fn download_breeze_tts_2(model_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_breeze_tts_2_assets(&model_dir, hf_token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_vox_cpm_2_ready(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_vox_cpm_2_ready(&model_dir))
}

#[tauri::command]
pub async fn check_vox_cpm_2_downloaded(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_vox_cpm_2_ready(&model_dir))
}

#[tauri::command]
pub async fn download_vox_cpm_2(model_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_vox_cpm_2_assets(&model_dir, hf_token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_pocket_tts_ready(voice: String, voice_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_pocket_tts_ready(&voice, &voice_dir))
}

#[tauri::command]
pub async fn download_pocket_tts(voice: String, voice_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_pocket_tts_assets(&voice, &voice_dir, hf_token)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_pocket_tts_voices(voice_dir: String) -> Result<Vec<voxctrl_tts::PocketTtsVoiceOption>, String> {
    Ok(voxctrl_tts::pocket_tts_voice_catalogue(&voice_dir))
}

/// Whether the Inflect-Micro-v2 ONNX engine was compiled into this build. The UI
/// uses this to warn that selecting the engine in a build without it will fail,
/// rather than letting the failure surface only on the first utterance.
#[tauri::command]
pub fn inflect_micro_available() -> bool {
    voxctrl_tts::INFLECT_MICRO_COMPILED
}

#[tauri::command]
pub async fn check_inflect_micro_downloaded(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_inflect_micro_downloaded(&model_dir))
}

#[tauri::command]
pub async fn download_inflect_micro(model_dir: String) -> Result<(), String> {
    voxctrl_tts::download_inflect_micro_assets(&model_dir)
        .await
        .map_err(|e| e.to_string())
}

/// Report the tensor names a downloaded Inflect-Micro-v2 export actually
/// declares. This is the diagnostic path for a graph whose naming doesn't match
/// what `inflect::model` binds against: it loads the graphs without building a
/// synthesis plan, so it still returns a useful answer when loading for playback
/// would fail.
#[tauri::command]
pub async fn inflect_micro_inspect(model_dir: String) -> Result<serde_json::Value, String> {
    #[cfg(feature = "inflect-micro")]
    {
        let dir = voxctrl_tts::inflect::resolve_model_dir(&model_dir);
        let signature = tokio::task::spawn_blocking(move || voxctrl_tts::inflect::model::inspect(&dir))
            .await
            .map_err(|e| format!("inspect task join: {e}"))?
            .map_err(|e| format!("{e:#}"))?;
        serde_json::to_value(signature).map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "inflect-micro"))]
    {
        let _ = model_dir;
        Err("This build was compiled without the `inflect-micro` feature.".to_string())
    }
}

/// The resolved, absolute path to the shared voice-cloning reference-clip
/// folder (Pocket-TTS, Breeze-TTS-2, VoxCPM2), for display in Settings.
#[tauri::command]
pub fn get_cloned_tts_voices_dir() -> String {
    voxctrl_tts::cloned_tts_voices_dir().display().to_string()
}
