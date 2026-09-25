//! First-run setup: status, model download, wizard and tab routing.

use tauri::{Emitter, Manager};
use tracing::info;

use super::*;

/// The keystroke-injection helper this session needs, if it is not installed.
///
/// The package step of the setup script is deliberately best-effort (a stale
/// mirror must not block the permission fix), so it really can leave a system
/// with working hotkeys and no way to type the transcription anywhere. That
/// failure is otherwise invisible: dictation appears to do nothing at all.
pub fn missing_injection_tool() -> Option<&'static str> {
    #[cfg(not(target_os = "linux"))]
    {
        None
    }

    #[cfg(target_os = "linux")]
    {
        let have = |name: &str| voxctrl_config::find_in_path(name).is_some();
        if have("wtype") || have("xdotool") {
            return None;
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            Some("wtype")
        } else {
            Some("xdotool")
        }
    }
}

/// Everything first-run setup depends on, in one call: how global shortcuts are
/// being delivered, whether text can be typed anywhere, and whether a speech
/// model is on disk.
#[derive(serde::Serialize)]
pub struct SetupStatusPayload {
    pub hotkeys: HotkeyStatusPayload,
    /// Shortcuts can fire right now. Mirrors `hotkeys.is_active` so the UI can
    /// gate on it without reaching into the nested payload.
    pub hotkeys_active: bool,
    pub model_ready: bool,
    pub model_size: String,
    /// The configured model downloads itself in the background at first launch.
    pub model_auto_downloads: bool,
    /// Name of the missing keystroke-injection helper, if any.
    pub missing_injection_tool: Option<String>,
    /// Graphical privilege escalation is available for the one-click install.
    pub pkexec_available: bool,
    /// Commands that install the host packages by hand, for machines with no
    /// polkit agent. Empty when the distro is unknown.
    pub manual_package_commands: String,
    pub is_complete: bool,
}

#[tauri::command]
pub async fn get_setup_status(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<SetupStatusPayload, String> {
    let hotkeys = hotkey_status(&state.hotkey_health);
    let hotkeys_active = hotkeys.is_active;

    let (model_size, model_dir, uses_whisper_model) = {
        let cfg = state.config.lock().await;
        let eng = &cfg.data.engine;
        (
            eng.whisper_cpp.model_size.clone(),
            eng.whisper_cpp.model_dir.clone(),
            eng.backend != voxctrl_config::BackendChoice::RemoteOpenAi
                && (eng.backend != voxctrl_config::BackendChoice::Moonshine
                    || !voxctrl_inference::MOONSHINE_COMPILED)
                && (eng.backend != voxctrl_config::BackendChoice::Parakeet
                    || !voxctrl_inference::PARAKEET_COMPILED),
        )
    };

    let model_ready = !uses_whisper_model
        || voxctrl_inference::whisper_cpp::is_model_downloaded(&model_size, &model_dir);
    let model_auto_downloads =
        uses_whisper_model && voxctrl_inference::whisper_cpp::is_small_auto_downloadable(&model_size);

    let missing_tool = missing_injection_tool();

    Ok(SetupStatusPayload {
        is_complete: hotkeys_active && model_ready && missing_tool.is_none(),
        hotkeys,
        hotkeys_active,
        model_ready,
        model_size,
        model_auto_downloads,
        missing_injection_tool: missing_tool.map(str::to_string),
        pkexec_available: crate::installer::command_exists("pkexec"),
        manual_package_commands: crate::installer::manual_setup_commands(
            crate::installer::detect_pkg_manager(),
        ),
    })
}

/// Download whichever speech model the config currently selects.
///
/// The setup window has no business knowing the model directory layout, and
/// asking the user to pick a model before they have used the app once is the
/// step this whole flow is trying to remove.
#[tauri::command]
pub async fn download_configured_model(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<(), String> {
    let (model_size, model_dir) = {
        let cfg = state.config.lock().await;
        (
            cfg.data.engine.whisper_cpp.model_size.clone(),
            cfg.data.engine.whisper_cpp.model_dir.clone(),
        )
    };
    voxctrl_inference::whisper_cpp::download_model(&model_size, &model_dir)
        .await
        .map_err(|e| format!("{e:#}"))
}

/// Re-open the first-run wizard on demand.
///
/// Setup is not a one-time event in practice: a user changes microphone, moves
/// to a machine with a GPU, or wants to redo a hotkey they regret — and, more
/// immediately, whoever is developing the wizard needs to see it without
/// hand-editing `setup_completed` out of their config file.
#[tauri::command]
pub async fn open_setup_wizard(app: tauri::AppHandle) -> Result<(), String> {
    crate::window::open_wizard_window(&app)
}

/// Mark the first-run wizard finished and get out of the way.
///
/// The flag is written here rather than through `save_config` so that closing
/// the wizard is one atomic step: the config the wizard has been editing all
/// along is already persisted, and this only flips the bit that decides
/// whether the wizard opens again on the next launch.
#[tauri::command]
pub async fn finish_setup_wizard(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
    open_settings: bool,
) -> Result<(), String> {
    let updated = {
        let mut guard = state.config.lock().await;
        guard.data.ui.setup_completed = true;
        guard.save().map_err(|e| e.to_string())?;
        guard.data.clone()
    };
    let _ = app.emit("config-changed", updated);
    info!("First-run setup wizard completed");

    if let Some(window) = app.get_webview_window(crate::window::WIZARD_WINDOW) {
        let _ = window.hide();
        let _ = window.close();
    }
    if open_settings {
        if let Err(e) = crate::window::open_settings_window(&app) {
            tracing::error!("Could not open Settings after setup: {e}");
        }
    }
    Ok(())
}

/// Open the settings window on a specific tab. Used by the setup window so
/// "choose a different model" lands the user on the right screen instead of
/// making them find it.
#[tauri::command]
pub async fn open_settings_tab(app: tauri::AppHandle, tab: String) -> Result<(), String> {
    let existed = app.get_webview_window("settings").is_some();
    let window = crate::window::open_settings_window(&app)?;
    let _ = window.emit("focus-settings-tab", tab.clone());

    // A window built just now has no listener registered yet, so the first
    // emit lands before anything is listening. Repeating it once the frontend
    // has had a moment to mount costs nothing — selecting the same tab twice
    // is idempotent — and is the difference between landing on the right tab
    // and landing on the default one.
    if !existed {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            let _ = window.emit("focus-settings-tab", tab);
        });
    }
    Ok(())
}
