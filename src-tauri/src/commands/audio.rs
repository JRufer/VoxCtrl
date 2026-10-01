//! Audio device listing and the Settings VU-meter monitor.

use std::sync::Arc;

use tauri::State;
use tracing::info;

use crate::state::AppState;

#[tauri::command]
pub async fn start_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(true);
    info!("Audio monitoring started");
    Ok(())
}

#[tauri::command]
pub async fn stop_monitoring_audio(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_monitoring(false);
    info!("Audio monitoring stopped");
    Ok(())
}

#[tauri::command]
pub async fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    let devices = voxctrl_audio::list_input_devices();
    Ok(devices
        .into_iter()
        .map(|d| AudioDeviceInfo { index: d.index, name: d.name })
        .collect())
}

#[derive(serde::Serialize)]
pub struct AudioDeviceInfo {
    pub index: u32,
    pub name: String,
}
