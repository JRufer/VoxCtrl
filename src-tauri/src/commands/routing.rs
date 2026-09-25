//! Output targets, hotkey bindings and Chat-target commands.

use std::sync::Arc;

use tauri::State;
use tracing::info;
use voxctrl_routing::{HotkeyBinding, OutputTarget};

use crate::state::AppState;

use super::*;

#[tauri::command]
pub async fn get_targets(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<OutputTarget>, String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::load_targets(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_targets(
    state: State<'_, Arc<AppState>>,
    targets: Vec<OutputTarget>,
) -> Result<(), String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::save_targets(&targets, &dir).map_err(|e| e.to_string())?;
    
    // Update the in-memory targets cache
    state.set_targets(targets.clone()).await;

    // Hot-reload the router
    state.router.reload(targets).await;
    info!("Targets saved and router reloaded");

    // Start listeners for any response pipes the save added.
    state.spawn_fifo_responders().await;

    Ok(())
}

#[tauri::command]
pub async fn get_bindings(
    _state: State<'_, Arc<AppState>>,
) -> Result<Vec<HotkeyBinding>, String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::load_bindings(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_bindings(
    state: State<'_, Arc<AppState>>,
    bindings: Vec<HotkeyBinding>,
) -> Result<(), String> {
    let dir = voxctrl_routing::config_dir();
    voxctrl_routing::save_bindings(&bindings, &dir).map_err(|e| e.to_string())?;
    *state.bindings.lock().await = bindings.clone();
    info!("Bindings saved");
    
    // Hot reload the bindings in the active listener threads, re-injecting the
    // stop key when it is currently held — `stop_key`'s arbiter decides that,
    // and a save must not take a grab it released or drop one it is using.
    let all_bindings = crate::stop_key::listener_bindings(&state, bindings.clone()).await;
    let reloader_guard = state.hotkey_reloader.lock().await;
    if let Some(reloader) = &*reloader_guard {
        if let Err(e) = reloader.send(all_bindings) {
            tracing::warn!("Failed to hot-reload bindings: {e}");
        } else {
            info!("Hot-reload signal sent to listener");
        }
    }
    
    // Where the desktop owns the key grab, the saved bindings have to be pushed
    // to it — nothing else will notice they changed. Narrowly gated: the Mint
    // route is only right when it is already in use, or when nothing else
    // worked at all. Registering it alongside a backend that watches the keys
    // itself would fire every shortcut twice, and `Starting` has not finished
    // deciding yet.
    let backend = state.hotkey_health.backend();
    if backend == voxctrl_hotkeys::Backend::MintDbus
        || (backend == voxctrl_hotkeys::Backend::None
            && crate::mint_shortcuts::is_mint_desktop())
    {
        match crate::mint_shortcuts::sync_mint_shortcuts(&bindings) {
            Ok(registered) => {
                state
                    .hotkey_health
                    .set_backend(voxctrl_hotkeys::Backend::MintDbus);
                info!(
                    "Mirrored {} binding(s) into Linux Mint's own shortcut settings",
                    registered.len()
                );
            }
            Err(e) => tracing::warn!("Failed to sync Linux Mint native shortcut settings: {e}"),
        }
    }

    Ok(())
}

/// Forget a Chat target's conversation so the next dictation starts fresh.
#[tauri::command]
pub async fn reset_chat_conversation(
    _state: State<'_, Arc<AppState>>,
    target_id: String,
) -> Result<usize, String> {
    let dropped = voxctrl_routing::reset_chat_history(&target_id).await;
    info!("Chat conversation for '{target_id}' reset ({dropped} messages dropped)");
    Ok(dropped)
}

/// Probe a Chat target's endpoint and list the models it serves.
///
/// Takes an unsaved target so the settings UI can test edits before persisting
/// them. Routed through Rust rather than `fetch` in the webview because the
/// endpoint is a third-party server that need not send CORS headers.
#[tauri::command]
pub async fn test_chat_target(target: OutputTarget) -> Result<OpenAiTestResult, String> {
    use voxctrl_config::OpenAiConfig;

    let endpoint = target.chat_url.unwrap_or_default();
    if endpoint.trim().is_empty() {
        return Err("No server URL configured.".into());
    }
    let client = voxctrl_llm::OpenAiClient::new(OpenAiConfig {
        enabled: true,
        endpoint: endpoint.clone(),
        api_key: target.chat_api_key,
        model: String::new(),
        timeout_secs: target.chat_timeout_secs.clamp(1, 30),
        ..Default::default()
    });

    match client.list_models().await {
        Ok(models) if models.is_empty() => Ok(OpenAiTestResult {
            success: true,
            message: format!("Connected to {endpoint}, but it reported no models."),
            models,
        }),
        Ok(models) => Ok(OpenAiTestResult {
            success: true,
            message: format!("Connected to {endpoint} — {} model(s) available.", models.len()),
            models,
        }),
        Err(e) => Ok(OpenAiTestResult {
            success: false,
            message: format!("Could not reach {endpoint}: {e}"),
            models: Vec::new(),
        }),
    }
}

/// Render a file target's timestamp format so the Settings UI can preview it
/// and report a bad pattern before the target is saved.
///
/// chrono is the authority on what a `strftime` pattern means, so the preview
/// comes from the same code the file target writes with rather than from a
/// second implementation in TypeScript.
#[tauri::command]
pub async fn preview_timestamp_format(format: String) -> Result<String, String> {
    voxctrl_routing::render_timestamp(&format, chrono::Utc::now())
}
