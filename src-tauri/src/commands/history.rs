//! Settings → History: listing, searching and reusing recent dictations.

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

/// Type an entry into whatever window had focus before Settings. Settings is
/// hidden first so focus returns there, then given a moment to settle.
#[tauri::command]
pub async fn paste_history_entry(
    state: State<'_, Arc<AppState>>,
    app: tauri::AppHandle,
    id: u64,
) -> Result<(), String> {
    let text = entry_text(&state, id)?;
    if let Some(win) = app.get_webview_window("settings") {
        let _ = win.hide();
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
