//! Text-to-speech: speaking, voices and TTS model downloads.

use std::sync::Arc;

use tauri::State;
use tracing::info;

use crate::state::AppState;

#[tauri::command]
pub async fn speak_text(state: State<'_, Arc<AppState>>, text: String, voice: Option<String>) -> Result<(), String> {
    info!("TTS speak_text via command: {} chars", text.chars().count());
    let handle = state.tts_handle.lock().await;
    if let Some(ref tts) = *handle {
        tts.speak_utterance(voxctrl_tts::Utterance { text, voice, source_label: None });
    }
    Ok(())
}

#[tauri::command]
pub async fn check_voice_downloaded(voice_name: String, voice_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_voice_downloaded(&voice_name, &voice_dir))
}

#[tauri::command]
pub async fn download_voice(voice_name: String, voice_dir: String) -> Result<(), String> {
    voxctrl_tts::download_voice(&voice_name, &voice_dir).await.map_err(|e| e.to_string())
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
    voxctrl_tts::download_breeze_tts_2_assets(&model_dir, hf_token).await.map_err(|e| e.to_string())
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
    voxctrl_tts::download_vox_cpm_2_assets(&model_dir, hf_token).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_kitten_tts_2_ready(model_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_kitten_tts_2_ready(&model_dir))
}

#[tauri::command]
pub async fn download_kitten_tts_2(model_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_kitten_tts_2_assets(&model_dir, hf_token).await.map_err(|e| e.to_string())
}

/// Kitten TTS 2's built-in preset voice names, for the Settings voice picker.
#[tauri::command]
pub fn list_kitten_tts_2_voices() -> Vec<&'static str> {
    voxctrl_tts::KITTEN_TTS_2_VOICES.to_vec()
}

/// Whether the Reverb-TTS model is downloaded. Whether the runtime can run it
/// is `reverb_runtime_supported`, which Settings shows separately. Takes the
/// audio.cpp server path as typed, which may not be saved yet.
#[tauri::command]
pub async fn check_reverb_ready(model_dir: String, audiocpp_server: Option<String>) -> Result<bool, String> {
    if let Some(path) = audiocpp_server.as_deref() {
        voxctrl_tts::set_audiocpp_server_override(path);
    }
    Ok(voxctrl_tts::is_reverb_downloaded(&model_dir))
}

/// Whether the configured audio.cpp runtime can run Reverb at all. No released
/// audio.cpp build ships the `reverb` family yet, so this is false until the
/// user points "audio.cpp server" at a build that has it.
#[tauri::command]
pub fn reverb_runtime_supported(audiocpp_server: Option<String>) -> bool {
    if let Some(path) = audiocpp_server.as_deref() {
        voxctrl_tts::set_audiocpp_server_override(path);
    }
    voxctrl_tts::reverb_runtime_supported()
}

#[tauri::command]
pub async fn download_reverb(model_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_reverb_assets(&model_dir, hf_token).await.map_err(|e| e.to_string())
}

/// Reverb-TTS's built-in preset voice ids, for the Settings voice picker.
#[tauri::command]
pub fn list_reverb_voices() -> Vec<&'static str> {
    voxctrl_tts::REVERB_VOICES.to_vec()
}

/// The language codes Reverb-TTS speaks (en, de, fr, es, it, pl, pt).
#[tauri::command]
pub fn list_reverb_languages() -> Vec<&'static str> {
    voxctrl_tts::REVERB_LANGUAGES.to_vec()
}

#[tauri::command]
pub async fn check_pocket_tts_ready(voice: String, voice_dir: String) -> Result<bool, String> {
    Ok(voxctrl_tts::is_pocket_tts_ready(&voice, &voice_dir))
}

#[tauri::command]
pub async fn download_pocket_tts(voice: String, voice_dir: String, hf_token: Option<String>) -> Result<(), String> {
    voxctrl_tts::download_pocket_tts_assets(&voice, &voice_dir, hf_token).await.map_err(|e| e.to_string())
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
    voxctrl_tts::download_inflect_micro_assets(&model_dir).await.map_err(|e| e.to_string())
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
/// folder (Pocket-TTS, Breeze-TTS-2, VoxCPM2, Kitten TTS 2, Reverb), for display in Settings.
#[tauri::command]
pub fn get_cloned_tts_voices_dir() -> String {
    voxctrl_tts::cloned_tts_voices_dir().display().to_string()
}

/// The Piper voices the picker should offer: the built-in catalogue plus any
/// model the user dropped into the voices folder (#190).
#[tauri::command]
pub async fn list_piper_voices(voice_dir: String) -> Result<Vec<voxctrl_tts::PiperVoiceOption>, String> {
    Ok(voxctrl_tts::piper_voice_catalogue(&voice_dir))
}

/// Whether the speech server at `endpoint` offers text-to-speech, and with
/// which engine and voice, in words for the Settings window. Takes the
/// endpoint and key as typed, which may not be saved yet.
#[tauri::command]
pub async fn check_server_tts(
    endpoint: String,
    api_key: Option<String>,
    tls_fingerprint: Option<String>,
) -> Result<voxctrl_tts::remote::ServerTtsStatus, String> {
    let link = voxctrl_tts::remote::ServerLink { endpoint, api_key, tls_fingerprint, ..Default::default() };
    // The client is blocking, for the TTS worker thread's sake.
    tokio::task::spawn_blocking(move || voxctrl_tts::remote::check_server_tts(&link)).await.map_err(|e| e.to_string())
}

/// What the TTS tab needs to offer this computer's own voices on a server.
#[derive(serde::Serialize)]
pub struct ServerVoiceSync {
    /// The voice clips on this computer that a server can take, by name.
    pub local: Vec<String>,
    pub uploaded: Vec<String>,
    pub removed: Vec<String>,
}

/// The names of this computer's own voice clips, for the server voice picker.
#[tauri::command]
pub async fn list_own_voices() -> Result<Vec<String>, String> {
    tokio::task::spawn_blocking(|| {
        voxctrl_tts::remote::local_custom_voices(&voxctrl_tts::cloned_tts_voices_dir())
            .into_iter()
            .map(|v| v.name)
            .collect()
    })
    .await
    .map_err(|e| e.to_string())
}

/// Make the server's copy of this computer's voice clips match what is here:
/// upload new and changed ones, remove ones deleted here. They are stored
/// under this client's id and no other client can use them.
#[tauri::command]
pub async fn sync_server_voices(
    endpoint: String,
    api_key: Option<String>,
    tls_fingerprint: Option<String>,
) -> Result<ServerVoiceSync, String> {
    tokio::task::spawn_blocking(move || {
        let link = voxctrl_tts::remote::ServerLink {
            endpoint,
            api_key,
            tls_fingerprint,
            client_id: voxctrl_tts::remote::default_client_id(),
            ..Default::default()
        };
        let local = voxctrl_tts::remote::local_custom_voices(&voxctrl_tts::cloned_tts_voices_dir());
        let report = voxctrl_tts::remote::sync_voices(&link, &local).map_err(|e| format!("{e:#}"))?;
        Ok(ServerVoiceSync {
            local: local.into_iter().map(|v| v.name).collect(),
            uploaded: report.uploaded,
            removed: report.removed,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}
