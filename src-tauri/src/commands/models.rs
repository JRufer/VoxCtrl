//! Speech-recognition model checks and downloads.

#[tauri::command]
pub async fn check_model_downloaded(model_size: String, model_dir: Option<String>) -> Result<bool, String> {
    let dir = model_dir.unwrap_or_default();
    Ok(voxctrl_inference::whisper_cpp::is_model_downloaded(&model_size, &dir))
}

#[tauri::command]
pub async fn download_model(model_size: String, model_dir: String) -> Result<(), String> {
    voxctrl_inference::whisper_cpp::download_model(&model_size, &model_dir)
        .await
        .map_err(|e| e.to_string())
}

/// Whether the Moonshine ONNX backend was compiled into this build. The UI uses
/// this to decide whether selecting Moonshine actually runs Moonshine (vs.
/// transparently falling back to whisper-cpp).
#[tauri::command]
pub fn moonshine_available() -> bool {
    voxctrl_inference::MOONSHINE_COMPILED
}

#[tauri::command]
pub async fn check_moonshine_downloaded(model_size: String) -> Result<bool, String> {
    #[cfg(feature = "moonshine")]
    {
        Ok(voxctrl_inference::moonshine::is_model_downloaded(&model_size, ""))
    }
    #[cfg(not(feature = "moonshine"))]
    {
        let _ = model_size;
        Ok(false)
    }
}

#[tauri::command]
pub async fn download_moonshine_model(model_size: String) -> Result<(), String> {
    #[cfg(feature = "moonshine")]
    {
        voxctrl_inference::moonshine::download_model(&model_size, "")
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "moonshine"))]
    {
        let _ = model_size;
        Err("This build was compiled without the Moonshine backend. Rebuild with `--features moonshine` to use it.".into())
    }
}

/// Whether the Parakeet ONNX backend was compiled into this build. The UI uses
/// this to decide whether selecting Parakeet actually runs Parakeet (vs.
/// transparently falling back to whisper-cpp).
#[tauri::command]
pub fn parakeet_available() -> bool {
    voxctrl_inference::PARAKEET_COMPILED
}

#[tauri::command]
pub async fn check_parakeet_downloaded(model_size: String) -> Result<bool, String> {
    #[cfg(feature = "parakeet")]
    {
        Ok(voxctrl_inference::parakeet::is_model_downloaded(&model_size, ""))
    }
    #[cfg(not(feature = "parakeet"))]
    {
        let _ = model_size;
        Ok(false)
    }
}

#[tauri::command]
pub async fn download_parakeet_model(model_size: String) -> Result<(), String> {
    #[cfg(feature = "parakeet")]
    {
        voxctrl_inference::parakeet::download_model(&model_size, "")
            .await
            .map_err(|e| e.to_string())
    }
    #[cfg(not(feature = "parakeet"))]
    {
        let _ = model_size;
        Err("This build was compiled without the Parakeet backend. Rebuild with `--features parakeet` to use it.".into())
    }
}

#[tauri::command]
pub async fn check_s1_mini_downloaded(model_dir: Option<String>) -> Result<bool, String> {
    Ok(voxctrl_inference::s1_mini::is_s1_mini_downloaded(model_dir.as_deref()))
}

#[tauri::command]
pub async fn download_s1_mini_model(model_dir: Option<String>) -> Result<(), String> {
    voxctrl_inference::s1_mini::download_s1_mini_assets(model_dir.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn check_directory_exists(path: String) -> Result<bool, String> {
    if path.is_empty() {
        return Ok(true);
    }
    Ok(expand_tilde(&path).is_dir())
}

fn expand_tilde(path: &str) -> std::path::PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(path)
}

// ── Model cleanup ─────────────────────────────────────────────────────────────

use std::sync::Arc;

use tauri::State;

use crate::model_cleanup::{self, CleanupPlan, CleanupResult, Layout};
use crate::state::AppState;

/// Every downloaded model, and which of them this machine still uses.
///
/// `fallback` is the fallback engine chosen in the dialog, which may not be
/// saved yet: the list shows what would be kept with that choice.
#[tauri::command]
pub async fn plan_model_cleanup(
    state: State<'_, Arc<AppState>>,
    fallback: voxctrl_config::FallbackChoice,
) -> Result<CleanupPlan, String> {
    let cfg = state.config.lock().await.data.clone();
    let bindings = state.bindings.lock().await.clone();
    tokio::task::spawn_blocking(move || model_cleanup::plan(&cfg, &bindings, fallback, &Layout::system()))
        .await
        .map_err(|e| e.to_string())
}

/// Delete the listed models. Only ids are accepted, never paths.
///
/// Goes by the saved settings, which the dialog writes before calling this,
/// so a model is kept if the saved fallback needs it even when the dialog
/// asked for it to go.
#[tauri::command]
pub async fn run_model_cleanup(
    state: State<'_, Arc<AppState>>,
    ids: Vec<String>,
) -> Result<CleanupResult, String> {
    let cfg = state.config.lock().await.data.clone();
    let bindings = state.bindings.lock().await.clone();
    let result = tokio::task::spawn_blocking(move || {
        let fallback = cfg.engine.remote_openai.fallback;
        let plan = model_cleanup::plan(&cfg, &bindings, fallback, &Layout::system());
        model_cleanup::execute(&plan, &ids)
    })
    .await
    .map_err(|e| e.to_string())?;
    tracing::info!(
        removed = ?result.removed,
        freed_bytes = result.freed_bytes,
        errors = ?result.errors,
        "model cleanup"
    );
    Ok(result)
}

/// Whether the speech server at `endpoint` offers dictation cleanup, in words
/// for the Settings window. Takes the endpoint and key as typed, which may
/// not be saved yet.
#[tauri::command]
pub async fn check_server_cleanup(
    endpoint: String,
    api_key: Option<String>,
    tls_fingerprint: Option<String>,
) -> Result<voxctrl_inference::remote_cleanup::ServerCleanupStatus, String> {
    Ok(voxctrl_inference::remote_cleanup::check_server_cleanup(
        &endpoint,
        api_key.as_deref(),
        tls_fingerprint.as_deref(),
    )
    .await)
}

/// What a pairing code from a server says, for the Settings window to fill
/// in. `typed_endpoint` supplies the server's address when the code has none
/// (a server in a container does not know its own).
#[derive(serde::Serialize)]
pub struct PairingDetails {
    pub endpoint: String,
    pub api_key: Option<String>,
    pub tls_fingerprint: Option<String>,
}

#[tauri::command]
pub fn read_pairing_code(code: String, typed_endpoint: String) -> Result<PairingDetails, String> {
    let pairing = voxctrl_pairing::PairingCode::decode(&code).map_err(|e| format!("{e:#}"))?;
    Ok(PairingDetails {
        endpoint: pairing.endpoint(&typed_endpoint).map_err(|e| format!("{e:#}"))?,
        api_key: pairing.api_key,
        tls_fingerprint: pairing.fingerprint,
    })
}
