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
