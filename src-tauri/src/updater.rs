//! The app's side of updating: when to check, what to show, and how to hand
//! over to the new build.
//!
//! A check happens when the user presses "Check for updates", when a script
//! calls `CheckForUpdate` over D-Bus, or — only if they turned on
//! `updates.check_automatically`, which is off by default — about a minute
//! after launch and then once a day. The automatic check never runs on an
//! installation a package manager owns, nor on a development build: see
//! [`voxctrl_update::InstallKind::checks_automatically`].
//!
//! The decisions that can be got wrong without a running desktop — is this
//! version newer, which file replaces this installation, is the download the
//! one GitHub published, has the user skipped it — live in `voxctrl-update` and
//! are tested there. What is here is the part that needs Tauri: the commands,
//! the schedule, the tray item and notification, the progress events, and the
//! restart.

use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use tauri::{Emitter, Manager, State};

use crate::state::AppState;
use voxctrl_update::{CheckOutcome, PendingUpdate, Progress, UpdateInfo};

/// The version this build reports — the same string the About tab shows, and
/// the one every release tag is compared against.
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How long after launch the first automatic check runs. Startup is busy —
/// the model is loading, the portal handshake is in flight — and a network
/// round-trip has no business competing with what the user is waiting for.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);

/// How often the schedule wakes to see whether a check is due. Short enough
/// that turning the setting on is noticed within minutes, without a restart.
const SCHEDULE_TICK: Duration = Duration::from_secs(15 * 60);

/// How long between automatic checks.
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// How often to look again, while the user is dictating, for a moment to say
/// that an update was found.
const IDLE_POLL: Duration = Duration::from_secs(5);

/// The running app, so a check started from D-Bus — which has no handle of
/// its own — can still update the tray. Set once during setup.
static APP: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Remember the app handle. Called once from setup.
pub fn init(app: &tauri::AppHandle) {
    let _ = APP.set(app.clone());
}

/// What the frontend gets back from a check.
#[derive(Debug, Clone, serde::Serialize)]
pub struct UpdateCheckPayload {
    /// The version running now.
    pub current_version: String,
    /// The update that was found, or `None` when this is the latest release.
    pub update: Option<UpdateInfo>,
}

/// Which release artifact this binary *is*, so the update keeps the variant
/// the user installed. On Windows the installer writes identical paths for the
/// CPU and GPU builds, so nothing on disk can answer this — only the features
/// the binary was compiled with.
fn is_gpu_build() -> bool {
    voxctrl_inference::moonshine_gpu_backend().is_some()
        || voxctrl_inference::whisper_gpu_backend().is_some()
        || voxctrl_inference::parakeet_gpu_backend().is_some()
}

/// Run a check the user asked for, remember the result, and reflect it in the
/// tray and on D-Bus. Returns what to show the user. A skipped version is still
/// shown here: someone who asked deserves the real answer.
pub async fn check(state: &Arc<AppState>) -> Result<UpdateCheckPayload, String> {
    let payload = fetch(state).await?;
    publish(payload.update.as_ref());
    Ok(payload)
}

/// Ask GitHub and remember what it said, without telling anyone.
async fn fetch(state: &Arc<AppState>) -> Result<UpdateCheckPayload, String> {
    let outcome = voxctrl_update::check(CURRENT_VERSION, is_gpu_build())
        .await
        .map_err(|e| e.to_string())?;

    match outcome {
        CheckOutcome::UpToDate { current } => {
            *state.pending_update.lock().await = None;
            Ok(UpdateCheckPayload { current_version: current, update: None })
        }
        CheckOutcome::Available(pending) => {
            let info = pending.info.clone();
            *state.pending_update.lock().await = Some(*pending);
            Ok(UpdateCheckPayload { current_version: CURRENT_VERSION.to_string(), update: Some(info) })
        }
    }
}

/// Show what a check found where it stays visible: the tray item, and the
/// `UpdateAvailable` D-Bus signal for scripts. `None` puts the tray item back to
/// "Check for updates".
fn publish(update: Option<&UpdateInfo>) {
    if let Some(app) = APP.get() {
        crate::tray::show_pending_update(app, update.map(|u| u.version.as_str()));
    }
    #[cfg(target_os = "linux")]
    if let Some(info) = update {
        crate::services::emit_update_available(info.version.clone());
    }
}

/// True while the user is dictating or VoxCtrl is reading aloud — not the
/// moment to put a notification on screen.
fn is_busy(state: &AppState) -> bool {
    state.is_recording() || state.is_processing() || state.is_speaking()
}

/// Run the automatic check on its schedule, for as long as the app runs.
///
/// Off unless the user turned on `updates.check_automatically`; the setting is
/// read on every tick, so turning it on or off takes effect without a restart.
/// Returns at once, for good, on an installation the automatic check must not
/// touch.
///
/// Every failure is silent by design. The user did not press anything; a
/// laptop on a train that cannot reach GitHub has nothing to apologise for.
/// Failures go to the log, and a manual check reports them properly.
pub fn spawn_automatic_checks(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        let kind = voxctrl_update::install::detect(is_gpu_build());
        if !kind.checks_automatically() {
            tracing::info!(
                "Automatic update checks never run on this installation ({kind:?}); \
                 a manual check still works"
            );
            return;
        }

        tokio::time::sleep(FIRST_CHECK_DELAY).await;

        let mut last_check: Option<Instant> = None;
        let mut announced: Option<String> = None;
        loop {
            let (enabled, setup_done, skipped) = {
                let cfg = state.config.lock().await;
                (
                    cfg.data.updates.check_automatically,
                    cfg.data.ui.setup_completed,
                    cfg.data.updates.skipped_version.clone(),
                )
            };
            let due = last_check.is_none_or(|t| t.elapsed() >= CHECK_INTERVAL);

            // Not during setup: a fresh install is on the latest release anyway.
            if enabled && setup_done && due && !state.is_updating() {
                last_check = Some(Instant::now());
                match fetch(&state).await {
                    Ok(UpdateCheckPayload { update: Some(info), .. }) => {
                        if !voxctrl_update::should_prompt(&info, skipped.as_deref()) {
                            tracing::info!("Update {} is available but was skipped", info.version);
                        } else {
                            publish(Some(&info));
                            if announced.as_deref() != Some(info.version.as_str()) {
                                announced = Some(info.version.clone());
                                announce(&state, &info).await;
                            }
                        }
                    }
                    Ok(UpdateCheckPayload { update: None, .. }) => {
                        publish(None);
                        tracing::info!("Automatic update check: VoxCtrl {CURRENT_VERSION} is up to date");
                    }
                    Err(e) => tracing::warn!("Automatic update check failed: {e}"),
                }
            }

            tokio::time::sleep(SCHEDULE_TICK).await;
        }
    });
}

/// Say once, with a desktop notification, that an update was found. Waits for
/// the user to finish dictating first. The tray item carries it from there; the
/// update window is never opened on top of what they are doing.
async fn announce(state: &AppState, info: &UpdateInfo) {
    while is_busy(state) {
        tokio::time::sleep(IDLE_POLL).await;
    }
    tracing::info!("Update available: {} → {}", info.current_version, info.version);
    voxctrl_inject::show_notification(
        &format!("VoxCtrl {} is available", info.version),
        "Open it from the VoxCtrl tray menu to see what's new and update.",
    );
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Check GitHub now, on the user's explicit say-so.
#[tauri::command]
pub async fn check_for_update(state: State<'_, Arc<AppState>>) -> Result<UpdateCheckPayload, String> {
    let state = state.inner().clone();
    check(&state).await
}

/// What the last check found, without asking GitHub again. This is what the
/// update window renders from, so opening it costs nothing.
#[tauri::command]
pub async fn get_pending_update(
    state: State<'_, Arc<AppState>>,
) -> Result<UpdateCheckPayload, String> {
    let pending = state.pending_update.lock().await;
    Ok(UpdateCheckPayload {
        current_version: CURRENT_VERSION.to_string(),
        update: pending.as_ref().map(|p| p.info.clone()),
    })
}

/// Download the pending update, install it, and restart into it.
///
/// Progress is emitted as `update-progress`; the app exits shortly after
/// `update-installed`. If anything fails the running installation is untouched
/// — nothing is replaced until a complete, checksum-verified file is sitting on
/// the same filesystem as the one it replaces.
#[tauri::command]
pub async fn install_update(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let state = state.inner().clone();

    let pending: PendingUpdate = {
        let guard = state.pending_update.lock().await;
        guard
            .clone()
            .ok_or_else(|| "There is no update to install. Check for updates first.".to_string())?
    };

    if !pending.info.can_self_update {
        return Err(pending
            .info
            .unsupported_reason
            .clone()
            .unwrap_or_else(|| "This installation cannot update itself.".to_string()));
    }

    if !state.begin_update() {
        return Err("An update is already being installed.".to_string());
    }

    let emitter = app.clone();
    let result = voxctrl_update::install(&pending, move |p: Progress| {
        let _ = emitter.emit("update-progress", p);
    })
    .await;

    let launch_path = match result {
        Ok(path) => path,
        Err(e) => {
            state.end_update();
            let message = e.to_string();
            tracing::error!("Update failed: {message}");
            let _ = app.emit("update-failed", message.clone());
            return Err(message);
        }
    };

    tracing::info!("Update installed; restarting into {}", launch_path.display());
    let _ = app.emit("update-installed", pending.info.version.clone());

    // A version the user has just installed is not one to keep skipping.
    {
        let mut cfg = state.config.lock().await;
        if cfg.data.updates.skipped_version.is_some() {
            cfg.data.updates.skipped_version = None;
            let _ = cfg.save();
        }
    }

    voxctrl_update::apply::spawn_relaunch(&launch_path).map_err(|e| {
        state.end_update();
        e.to_string()
    })?;

    // Long enough for the window to paint "restarting…", short enough that the
    // relaunch helper is not left polling for a process that will not die.
    // The helper waits for this process to exit before starting the new one,
    // so the single-instance guard cannot bounce the new copy off the old.
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(600)).await;
        app.exit(0);
    });

    Ok(())
}

/// Whether automatic checks can run on this installation at all — false on a
/// package-manager or development install — so Settings and the setup wizard
/// can say so instead of offering a switch that does nothing.
#[tauri::command]
pub fn automatic_update_checks_supported() -> bool {
    voxctrl_update::install::detect(is_gpu_build()).checks_automatically()
}

/// Stop raising this particular version in automatic checks. A newer one is
/// still raised, and a manual check still shows it.
#[tauri::command]
pub async fn skip_update_version(
    app: tauri::AppHandle,
    state: State<'_, Arc<AppState>>,
    version: String,
) -> Result<(), String> {
    {
        let mut cfg = state.config.lock().await;
        cfg.data.updates.skipped_version = Some(version.clone());
        cfg.save().map_err(|e| e.to_string())?;
        let _ = app.emit("config-changed", cfg.data.clone());
    }
    crate::tray::show_pending_update(&app, None);
    tracing::info!("Skipping update {version} at the user's request");
    Ok(())
}

/// Close the update window without doing anything. The pending update stays
/// remembered, and the tray item keeps offering it.
#[tauri::command]
pub async fn dismiss_update(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(crate::window::UPDATE_WINDOW) {
        let _ = window.close();
    }
    Ok(())
}

/// Open the update window from Settings, once a check has found something.
#[tauri::command]
pub async fn open_update_window(app: tauri::AppHandle) -> Result<(), String> {
    crate::window::open_update_window(&app)
}
