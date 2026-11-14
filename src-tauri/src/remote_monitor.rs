//! Keeps watch on the remote speech server while one is selected.
//!
//! Two small loops. One asks the server every few seconds whether it is
//! there, so the app usually knows it has gone before a dictation finds out
//! the hard way — which lets that dictation go straight to the fallback engine
//! instead of waiting on a connection first. The other reacts when the answer
//! changes: it tells the windows, rewrites the tray tooltip, and notifies the
//! user once when the server is lost.
//!
//! The answer itself lives in `voxctrl_inference::remote_status`, where the
//! recording pipeline and the inference worker read it too.

use std::sync::Arc;
use std::time::Duration;

use tauri::Emitter;
use voxctrl_config::{AppConfig, BackendChoice, FallbackChoice};
use voxctrl_inference::remote_status::{self, RemoteState, RemoteStatus};

use crate::state::AppState;

/// How often the server is asked. Short enough that the offline indicator
/// clears soon after a server comes back; long enough to be no load on it.
const PROBE_INTERVAL: Duration = Duration::from_secs(15);

static RECHECK: tokio::sync::Notify = tokio::sync::Notify::const_new();

/// Ask the server again now instead of at the next interval. Called when the
/// settings are saved, so a corrected address or key shows its effect at once.
pub fn recheck() {
    RECHECK.notify_one();
}

/// What the frontend is told: the status, plus what happens to a dictation
/// while the server is away.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct RemoteStatusPayload {
    pub state: RemoteState,
    pub reason: Option<String>,
    /// The fallback engine's name, or `None` when no fallback is set.
    pub fallback: Option<String>,
}

impl RemoteStatusPayload {
    pub fn new(status: RemoteStatus, cfg: &AppConfig) -> Self {
        Self {
            state: status.state,
            reason: status.reason,
            fallback: fallback_label(cfg.engine.remote_openai.fallback, cfg),
        }
    }
}

/// The fallback engine as the user knows it: engine and model.
pub fn fallback_label(choice: FallbackChoice, cfg: &AppConfig) -> Option<String> {
    match choice {
        FallbackChoice::None => None,
        FallbackChoice::WhisperCpp => Some(format!("Whisper.cpp {}", cfg.engine.whisper_cpp.model_size)),
        FallbackChoice::Moonshine => Some(format!("Moonshine {}", cfg.engine.moonshine.model_size)),
        FallbackChoice::Parakeet => Some(format!("Parakeet {}", cfg.engine.parakeet.model_size)),
    }
}

/// The notification shown once when the server is lost: `(title, body)`.
pub fn offline_notification(reason: Option<&str>, fallback: Option<&str>) -> (String, String) {
    let reason = reason.map(str::trim).filter(|r| !r.is_empty()).unwrap_or("The server could not be reached");
    let consequence = match fallback {
        Some(engine) => format!("Dictation will use the fallback engine ({engine}) until it is back."),
        None => "Dictation will fail until it is back: no fallback engine is set in Settings → Engine.".to_string(),
    };
    ("VoxCtrl — speech server unreachable".to_string(), format!("{reason}. {consequence}"))
}

/// Whether a change of status is the moment to notify: the server has just
/// been lost. Staying offline (a new reason, a later probe) is not news.
pub fn should_notify(previous: RemoteState, now: RemoteState) -> bool {
    now == RemoteState::Offline && previous != RemoteState::Offline
}

pub fn spawn(app: tauri::AppHandle, state: Arc<AppState>) {
    let probe_state = state.clone();
    tokio::spawn(async move {
        // The server last asked, so that pointing the app at a different one
        // (or changing the key) forgets what was known about the old one.
        let mut watched: Option<(String, Option<String>)> = None;
        loop {
            let remote = {
                let cfg = probe_state.config.lock().await;
                (cfg.data.engine.backend == BackendChoice::RemoteOpenAi)
                    .then(|| cfg.data.engine.remote_openai.clone())
            };
            match remote {
                None => {
                    watched = None;
                    remote_status::set_not_in_use();
                }
                Some(remote) => {
                    let key = (remote.endpoint.clone(), remote.api_key.clone());
                    if watched.as_ref() != Some(&key) {
                        remote_status::set_unknown();
                        watched = Some(key);
                    }
                    match voxctrl_inference::probe_remote_speech_engine(&remote.endpoint, remote.api_key.as_deref())
                        .await
                    {
                        Ok(()) => remote_status::set_online(),
                        Err(reason) => remote_status::set_offline(reason),
                    }
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(PROBE_INTERVAL) => {}
                _ = RECHECK.notified() => {}
            }
        }
    });

    tokio::spawn(async move {
        let mut changes = remote_status::subscribe();
        let mut previous = changes.borrow_and_update().state;
        while changes.changed().await.is_ok() {
            let status = changes.borrow_and_update().clone();
            let payload = {
                let cfg = state.config.lock().await;
                // Whether live text can be produced follows the server, and the
                // overlays choose their layout by it: without this they would
                // keep an empty text area through an outage.
                state.set_live_text_capable(crate::pipeline::live_text_capable(&cfg.data));
                RemoteStatusPayload::new(status, &cfg.data)
            };

            match payload.state {
                RemoteState::Offline => tracing::warn!(
                    "Remote speech server unreachable: {}",
                    payload.reason.as_deref().unwrap_or("no reason given")
                ),
                RemoteState::Online if previous == RemoteState::Offline => {
                    tracing::info!("Remote speech server is reachable again")
                }
                _ => {}
            }

            crate::tray::update_tray_for_remote(
                &app,
                (payload.state == RemoteState::Offline).then(|| payload.reason.clone().unwrap_or_default()),
            );
            if should_notify(previous, payload.state) {
                let (title, body) = offline_notification(payload.reason.as_deref(), payload.fallback.as_deref());
                voxctrl_inject::show_notification(&title, &body);
            }
            let _ = app.emit("remote-status", &payload);
            previous = payload.state;
        }
    });
}
