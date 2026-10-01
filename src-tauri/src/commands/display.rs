//! Monitor listing and the overlay window handshake.

use tauri::Manager;

#[derive(serde::Serialize)]
pub struct MonitorInfo {
    pub name: Option<String>,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
}

#[tauri::command]
pub async fn get_available_monitors(app: tauri::AppHandle) -> Result<Vec<MonitorInfo>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    let app_handle = app.clone();
    let res = app.run_on_main_thread(move || {
        let mut list = Vec::new();
        if let Some(w) = app_handle.webview_windows().values().next() {
            if let Ok(monitors) = w.available_monitors() {
                let primary = w.primary_monitor().ok().flatten();
                let primary_name = primary.as_ref().and_then(|m| m.name());

                for m in monitors {
                    let name = m.name().map(|s| s.to_string());
                    let is_primary = primary_name.is_some() && name.as_deref() == primary_name.map(|s| s.as_ref());
                    let size = m.size();
                    list.push(MonitorInfo {
                        name,
                        width: size.width,
                        height: size.height,
                        is_primary,
                    });
                }
            }
        }
        let _ = tx.send(list);
    });

    if let Err(e) = res {
        return Err(format!("Failed to run monitor query on main thread: {}", e));
    }

    rx.await.map_err(|e| format!("Failed to receive monitors: {}", e))
}

/// The overlay's frontend reporting that it has painted a frame.
///
/// The overlay window is built fully transparent so the user never sees it
/// during the tens-to-hundreds of milliseconds it takes to create the
/// webview, load `/overlay` and mount — see `window::reveal_overlay`. This is
/// what takes it off the safety-net timeout and shows it as soon as there is
/// actually something to look at.
#[tauri::command]
pub fn overlay_content_ready(app: tauri::AppHandle) {
    crate::window::reveal_overlay(&app);
}
