//! Custom overlay discovery.

/// The resolved, absolute path to the custom-overlays folder, for display
/// in Settings (see `custom_overlays::overlays_dir`).
#[tauri::command]
pub fn get_custom_overlays_dir() -> String {
    crate::custom_overlays::overlays_dir().display().to_string()
}

#[derive(serde::Serialize)]
pub struct CustomOverlayInfo {
    pub name: String,
    pub html: String,
    pub css: String,
}

/// The display name a custom-overlay folder is exposed under: unchanged,
/// unless it collides with a built-in style's internal name, in which case
/// `_custom` is appended so it stays selectable without clashing.
fn custom_overlay_display_name(folder_name: &str) -> String {
    const RESERVED: &[&str] = &[
        "waveform", "pulse", "blue_wave", "voice_card", "none",
        "mono_bars", "spectrum", "terminal", "vinyl",
    ];
    if RESERVED.contains(&folder_name.to_lowercase().as_str()) {
        format!("{folder_name}_custom")
    } else {
        folder_name.to_string()
    }
}

fn read_custom_overlay_folder(dir: &std::path::Path, display_name: String) -> CustomOverlayInfo {
    let html = std::fs::read_to_string(dir.join("index.html")).unwrap_or_default();
    let css = std::fs::read_to_string(dir.join("style.css")).unwrap_or_default();
    CustomOverlayInfo { name: display_name, html, css }
}

/// Every custom overlay's name, HTML and CSS — used to populate the
/// selectable style list in Settings → Visual & Feedback (`VisualTab.svelte`).
/// Rendering the *active* style, live, goes through `get_custom_overlay`
/// instead: reading every overlay's files here just to display one would
/// mean an edit to an overlay that isn't even selected still costs a disk
/// read on every style switch, for no reason.
#[tauri::command]
pub async fn get_custom_overlays() -> Result<Vec<CustomOverlayInfo>, String> {
    let overlays_dir = crate::custom_overlays::overlays_dir();

    if !overlays_dir.exists() {
        let _ = std::fs::create_dir_all(&overlays_dir);
    }

    let mut list = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&overlays_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let folder_name = entry.file_name().to_string_lossy().to_string();

                    // Filter out legacy gradient-wave
                    if folder_name.to_lowercase() == "gradient-wave" || folder_name.to_lowercase() == "gradient_wave" {
                        continue;
                    }

                    let display_name = custom_overlay_display_name(&folder_name);
                    list.push(read_custom_overlay_folder(&entry.path(), display_name));
                }
            }
        }
    }

    Ok(list)
}

/// One custom overlay's HTML and CSS, read fresh from disk, by the display
/// name it's selected under (`config.ui.overlay_style`). Used by
/// `Overlay.svelte` to (re-)read the active style's files at the moment it's
/// selected — including re-selecting it after editing its files — rather
/// than caching content from app startup, so on-disk edits are picked up
/// without an app restart. Returns `None` when no folder resolves to this
/// name (a built-in style, or a deleted/renamed custom one).
#[tauri::command]
pub async fn get_custom_overlay(name: String) -> Result<Option<CustomOverlayInfo>, String> {
    let overlays_dir = crate::custom_overlays::overlays_dir();

    if let Ok(entries) = std::fs::read_dir(&overlays_dir) {
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    let folder_name = entry.file_name().to_string_lossy().to_string();
                    if custom_overlay_display_name(&folder_name) == name {
                        return Ok(Some(read_custom_overlay_folder(&entry.path(), name)));
                    }
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_custom_overlays_returns_list() {
        let result = get_custom_overlays().await;
        assert!(result.is_ok());
        if let Ok(list) = result {
            // Check that the list is serializable
            let json = serde_json::to_string(&list);
            assert!(json.is_ok());
        }
    }
}
