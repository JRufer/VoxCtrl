//! Connection tests for the OpenAI-compatible and remote speech endpoints.

#[derive(serde::Serialize)]
pub struct OpenAiTestResult {
    pub success: bool,
    pub message: String,
    pub models: Vec<String>,
}

#[tauri::command]
pub async fn test_openai(
    endpoint: String,
    api_key: Option<String>,
    timeout_secs: u64,
) -> Result<OpenAiTestResult, String> {
    use voxctrl_config::OpenAiConfig;
    let cfg = OpenAiConfig {
        enabled: true,
        endpoint: endpoint.clone(),
        api_key,
        model: String::new(),
        timeout_secs,
        ..Default::default()
    };
    let client = voxctrl_llm::OpenAiClient::new(cfg);
    if client.is_available().await {
        match client.list_models().await {
            Ok(models) => {
                Ok(OpenAiTestResult {
                    success: true,
                    message: "Successfully connected to the OpenAI API server!".to_string(),
                    models,
                })
            }
            Err(e) => {
                Ok(OpenAiTestResult {
                    success: true,
                    message: format!("Successfully connected, but failed to fetch model list: {}", e),
                    models: Vec::new(),
                })
            }
        }
    } else {
        Ok(OpenAiTestResult {
            success: false,
            message: format!("Failed to connect to the OpenAI API server at '{}'. Check the URL and API key.", endpoint),
            models: Vec::new(),
        })
    }
}

#[tauri::command]
pub async fn test_remote_stt(
    endpoint: String,
    api_key: Option<String>,
    model: String,
    timeout_secs: u64,
) -> Result<voxctrl_inference::RemoteSttTestResult, String> {
    voxctrl_inference::test_remote_speech_engine(
        &endpoint,
        api_key.as_deref(),
        &model,
        timeout_secs,
    )
    .await
    .map_err(|e| e.to_string())
}
