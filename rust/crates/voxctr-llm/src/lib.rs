use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tracing::{debug, info, warn};
use voxctr_config::{OllamaConfig, OllamaMode};

// ── Prompts per mode ──────────────────────────────────────────────────────────

fn mode_prompt(mode: &OllamaMode, text: &str) -> String {
    match mode {
        OllamaMode::Clean => format!(
            "Fix grammar and punctuation only. Return only the corrected text, no commentary.\n\nText: {text}"
        ),
        OllamaMode::Formal => format!(
            "Rewrite in formal professional language. Return only the result.\n\nText: {text}"
        ),
        OllamaMode::Casual => format!(
            "Rewrite in casual conversational language. Return only the result.\n\nText: {text}"
        ),
        OllamaMode::Bullet => format!(
            "Convert to a bullet-point list. Return only the list.\n\nText: {text}"
        ),
        OllamaMode::Concise => format!(
            "Summarize concisely in 1-2 sentences. Return only the summary.\n\nText: {text}"
        ),
        OllamaMode::Custom => text.to_string(), // handled separately
    }
}

// ── Ollama API types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: &'a str,
    stream: bool,
}

#[derive(Deserialize)]
struct GenerateResponse {
    response: String,
}

// ── Client ────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct OllamaClient {
    config: OllamaConfig,
    http: reqwest::Client,
    available: std::sync::Arc<std::sync::Mutex<Option<bool>>>,
}

impl OllamaClient {
    pub fn new(config: OllamaConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("reqwest client");
        Self {
            config,
            http,
            available: std::sync::Arc::new(std::sync::Mutex::new(None)),
        }
    }

    /// Lazily probe if Ollama is reachable. Cached after first check.
    pub async fn is_available(&self) -> bool {
        {
            let guard = self.available.lock().unwrap();
            if let Some(v) = *guard {
                return v;
            }
        }
        let url = format!("{}/api/tags", self.config.endpoint);
        let ok = self
            .http
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        *self.available.lock().unwrap() = Some(ok);
        if ok {
            info!("Ollama reachable at {}", self.config.endpoint);
        } else {
            warn!("Ollama not reachable at {}", self.config.endpoint);
        }
        ok
    }

    /// Post-process text through Ollama. Returns original text on any failure.
    pub async fn process(&self, text: &str) -> String {
        if !self.config.enabled {
            return text.to_string();
        }
        if !self.is_available().await {
            return text.to_string();
        }

        let prompt = if self.config.mode == OllamaMode::Custom {
            if let Some(tmpl) = &self.config.custom_prompt {
                tmpl.replace("{text}", text)
            } else {
                return text.to_string();
            }
        } else {
            mode_prompt(&self.config.mode, text)
        };

        let url = format!("{}/api/generate", self.config.endpoint);
        let req = GenerateRequest {
            model: &self.config.model,
            prompt: &prompt,
            stream: false,
        };

        match self.http.post(&url).json(&req).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<GenerateResponse>().await {
                    Ok(body) => {
                        let result = body.response.trim().to_string();
                        debug!(
                            input_len = text.len(),
                            output_len = result.len(),
                            "Ollama processed"
                        );
                        result
                    }
                    Err(e) => {
                        warn!("Ollama response parse error: {e}");
                        text.to_string()
                    }
                }
            }
            Ok(resp) => {
                warn!("Ollama HTTP {}", resp.status());
                text.to_string()
            }
            Err(e) => {
                warn!("Ollama request error: {e}");
                text.to_string()
            }
        }
    }

    /// Reset the availability cache (e.g., user changed endpoint in settings).
    pub fn reset_availability(&self) {
        *self.available.lock().unwrap() = None;
    }
}
