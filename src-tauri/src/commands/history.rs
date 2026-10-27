//! The History window (tray → History): listing, searching and reusing
//! recent dictations.

use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::history::HistoryStatus;
use crate::state::AppState;

#[derive(Serialize)]
pub struct HistoryView {
    pub entries: Vec<voxctrl_history::Entry>,
    pub status: HistoryStatus,
}

/// How many entries the History tab lists at most.
const LIST_LIMIT: usize = 1000;

#[tauri::command]
pub fn get_history(state: State<'_, Arc<AppState>>, query: Option<String>) -> HistoryView {
    HistoryView {
        entries: state.history.search(query.as_deref().unwrap_or(""), LIST_LIMIT),
        status: state.history.status(),
    }
}

#[tauri::command]
pub fn delete_history_entry(state: State<'_, Arc<AppState>>, app: tauri::AppHandle, id: u64) -> bool {
    let removed = state.history.delete(id);
    let _ = app.emit("history-changed", ());
    removed
}

#[tauri::command]
pub fn clear_history(state: State<'_, Arc<AppState>>, app: tauri::AppHandle) {
    state.history.clear();
    let _ = app.emit("history-changed", ());
}

fn entry_text(state: &AppState, id: u64) -> Result<String, String> {
    state
        .history
        .get(id)
        .map(|e| e.text)
        .ok_or_else(|| "That dictation is no longer in the history.".to_string())
}

/// Type an entry into whatever window had focus before VoxCtrl's. The History
/// window (and Settings, if open) is hidden first so focus returns there,
/// then given a moment to settle.
#[tauri::command]
pub async fn paste_history_entry(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    id: u64,
) -> Result<(), String> {
    let text = entry_text(&state, id)?;
    for label in [crate::window::HISTORY_WINDOW, "settings"] {
        if let Some(win) = app.get_webview_window(label) {
            let _ = win.hide();
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    voxctrl_inject::inject_text(&text).await.map_err(|e| e.to_string())
}

/// Deliver an entry again, to any output target.
#[tauri::command]
pub async fn send_history_entry(
    state: State<'_, Arc<AppState>>,
    id: u64,
    target_id: String,
) -> Result<(), String> {
    let text = entry_text(&state, id)?;
    state.router.deliver_direct(&target_id, &text).await;
    Ok(())
}

/// Read an entry aloud. Queues it and returns; the window follows progress
/// through the `tts-playback-start` / `tts-playback-end` / `tts-error`
/// events. Fails straight away when text-to-speech isn't running, so the
/// button never waits for playback that will never start.
#[tauri::command]
pub async fn speak_history_entry(state: State<'_, Arc<AppState>>, id: u64) -> Result<(), String> {
    let text = entry_text(&state, id)?;
    let handle = state.tts_handle.lock().await;
    let Some(tts) = handle.as_ref() else {
        return Err("Text-to-speech is off. Turn it on in Settings → TTS.".into());
    };
    tts.speak_utterance(voxctrl_tts::Utterance { text, voice: None, source_label: None });
    Ok(())
}

/// Open the History window (tray → History).
#[tauri::command]
pub fn open_history(app: tauri::AppHandle) -> Result<(), String> {
    crate::window::open_history_window(&app)
}
