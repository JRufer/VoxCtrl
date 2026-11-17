use std::time::Duration;

use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};
use voxctrl_config::{OpenAiConfig, OpenAiMode, RemoteOpenAiConfig};

// ── Preset system prompts ───────────────────────────────────────────────────

/// System-prompt text for each built-in preset. `Custom` has no preset and
/// returns an empty string (the user prompt carries the full instruction).
pub fn preset_system_prompt(mode: &OpenAiMode) -> &'static str {
    match mode {
        OpenAiMode::Clean => "Fix grammar and punctuation only. Return only the corrected text, no commentary.",
        OpenAiMode::Formal => "Rewrite the user's text in formal professional language. Return only the result.",
        OpenAiMode::Casual => "Rewrite the user's text in casual conversational language. Return only the result.",
        OpenAiMode::Bullet => "Convert the user's text to a bullet-point list. Return only the list.",
        OpenAiMode::Concise => "Summarize the user's text concisely in 1-2 sentences. Return only the summary.",
        OpenAiMode::Custom => "",
    }
}

// ── OpenAI-compatible API types ─────────────────────────────────────────────

#[derive(Serialize)]
struct ChatMessage<'a> {
    role: &'a str,
    content: &'a str,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: Vec<ChatMessage<'a>>,
    stream: bool,
}

#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

/// Normalize a user-supplied endpoint into an OpenAI-style base URL ending in
/// `/v1`. Accepts values with or without a trailing slash or existing `/v1`.
fn api_base(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

/// How long to wait for the API server to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct OpenAiClient {
    config: OpenAiConfig,
    http: reqwest::Client,
    /// Why no request may be sent with these settings, when that is so: a
    /// server paired with encryption but given an unencrypted address.
    refused: Option<String>,
}

impl OpenAiClient {
    pub fn new(config: OpenAiConfig) -> Self {
        let http = reqwest::Client::builder()
            // An unreachable server fails fast here rather than after the full
            // request timeout, which is what lets `process` skip a separate
            // reachability probe (a whole extra round trip per rewrite).
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .expect("reqwest client");
        Self { config, http, refused: None }
    }

    /// A client whose requests go through a VoxCtrl server, which answers
    /// them with its own language model. The prompts and timeout stay as
    /// `config` has them; the address, key and certificate are the server's.
    pub fn via_server(mut config: OpenAiConfig, server: &RemoteOpenAiConfig) -> Self {
        config.endpoint = server_base(&server.endpoint);
        config.api_key = server.api_key.clone();
        let builder = reqwest::Client::builder()
            // A server on the network gets a little longer than a local one.
            .connect_timeout(SERVER_CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(config.timeout_secs));
        match voxctrl_pairing::pin(builder, &server.endpoint, server.tls_fingerprint.as_deref())
            .and_then(|b| b.build().map_err(Into::into))
        {
            Ok(http) => Self { config, http, refused: None },
            Err(e) => Self { config, http: reqwest::Client::new(), refused: Some(format!("{e:#}")) },
        }
    }

    /// Attach the configured API key as a Bearer token, if one is set.
    fn with_auth(&self, req: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.config.api_key.as_deref() {
            Some(key) if !key.trim().is_empty() => req.bearer_auth(key.trim()),
            _ => req,
        }
    }

    /// Probe whether the API server is reachable (used by the Settings test).
    pub async fn is_available(&self) -> bool {
        let url = format!("{}/models", api_base(&self.config.endpoint));
        let ok = self
            .with_auth(self.http.get(&url))
            .timeout(CONNECT_TIMEOUT)
            .send()
            .await
            .map(|r| r.status().is_success())
            .unwrap_or(false);
        if ok {
            info!("OpenAI API reachable at {}", self.config.endpoint);
        } else {
            warn!("OpenAI API not reachable at {}", self.config.endpoint);
        }
        ok
    }

    /// Post-process text through the OpenAI API. Returns original text on any failure.
    ///
    /// No reachability probe first: an unreachable server already fails within
    /// [`CONNECT_TIMEOUT`] and falls back to the original text below, so a
    /// probe would only add a round trip to every rewrite that succeeds.
    pub async fn process(&self, text: &str) -> String {
        if !self.config.enabled {
            return text.to_string();
        }
        if let Some(reason) = &self.refused {
            warn!("AI processing skipped: {reason}");
            return text.to_string();
        }

        // Build the user message from the configured template, substituting the
        // dictated text into the "{text}" placeholder.
        let template =
            if self.config.user_prompt.trim().is_empty() { "{text}" } else { self.config.user_prompt.as_str() };
        let user_content = if template.contains("{text}") {
            template.replace("{text}", text)
        } else {
            // Be forgiving if the user dropped the placeholder: append the text.
            format!("{template}\n\n{text}")
        };

        let system_content = self.config.system_prompt.trim();
        let mut messages = Vec::with_capacity(2);
        if !system_content.is_empty() {
            messages.push(ChatMessage { role: "system", content: system_content });
        }
        messages.push(ChatMessage { role: "user", content: &user_content });

        let url = format!("{}/chat/completions", api_base(&self.config.endpoint));
        let req = ChatRequest { model: &self.config.model, messages, stream: false };

        match self.with_auth(self.http.post(&url).json(&req)).send().await {
            Ok(resp) if resp.status().is_success() => match resp.json::<ChatResponse>().await {
                Ok(body) => {
                    let result = body
                        .choices
                        .into_iter()
                        .next()
                        .map(|c| c.message.content.trim().to_string())
                        .unwrap_or_default();
                    if result.is_empty() {
                        warn!("OpenAI API returned no content");
                        return text.to_string();
                    }
                    debug!(input_len = text.len(), output_len = result.len(), "OpenAI API processed");
                    result
                }
                Err(e) => {
                    warn!("OpenAI API response parse error: {e}");
                    text.to_string()
                }
            },
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if status.as_u16() == 404 {
                    warn!(
                        "OpenAI API HTTP 404 for model '{}': {body}. \
                         The server (e.g. Ollama) likely doesn't have this model — \
                         pull it or pick an available model in Settings.",
                        self.config.model
                    );
                } else {
                    warn!("OpenAI API HTTP {status}: {body}");
                }
                text.to_string()
            }
            Err(e) if voxctrl_pairing::is_certificate_mismatch(&e) => {
                warn!("AI processing skipped: {}", voxctrl_pairing::CERTIFICATE_MISMATCH_HELP);
                text.to_string()
            }
            Err(e) => {
                warn!("OpenAI API request error: {e}");
                text.to_string()
            }
        }
    }

    /// Retrieve the list of available models from the OpenAI-compatible server.
    pub async fn list_models(&self) -> Result<Vec<String>, String> {
        let url = format!("{}/models", api_base(&self.config.endpoint));
        let resp = self.with_auth(self.http.get(&url)).send().await.map_err(|e| e.to_string())?;
        if resp.status().is_success() {
            #[derive(serde::Deserialize)]
            struct ModelItem {
                id: String,
            }
            #[derive(serde::Deserialize)]
            struct ModelsResponse {
                data: Vec<ModelItem>,
            }
            let body = resp.json::<ModelsResponse>().await.map_err(|e| e.to_string())?;
            Ok(body.data.into_iter().map(|m| m.id).collect())
        } else {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            Err(format!("HTTP {status}: {body}"))
        }
    }
}

// ── AI processing through a VoxCtrl server ──────────────────────────────────

/// How long a VoxCtrl server gets to accept a connection.
const SERVER_CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// The server's `/v1` base for a speech endpoint, however it was entered.
fn server_base(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    api_base(trimmed.strip_suffix("/audio/transcriptions").unwrap_or(trimmed))
}

/// Whether a server does AI processing, in words for the Settings window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerLlmStatus {
    pub available: bool,
    /// The language model the server answers with, when it says.
    pub model: Option<String>,
    /// What to tell the user, including what to do when it is not available.
    pub message: String,
}

impl ServerLlmStatus {
    fn unavailable(message: impl Into<String>) -> Self {
        Self { available: false, model: None, message: message.into() }
    }
}

/// Ask the server whether it does AI processing. Never an error: every way
/// this can go wrong is something to show the user.
pub async fn check_server_llm(endpoint: &str, api_key: Option<&str>, tls_fingerprint: Option<&str>) -> ServerLlmStatus {
    if endpoint.trim().is_empty() {
        return ServerLlmStatus::unavailable(
            "No server is set up yet. Enter its address under Engine, with Remote Speech Engine as the backend.",
        );
    }
    let builder = reqwest::Client::builder().connect_timeout(SERVER_CONNECT_TIMEOUT).timeout(Duration::from_secs(6));
    let client =
        match voxctrl_pairing::pin(builder, endpoint, tls_fingerprint).and_then(|b| b.build().map_err(Into::into)) {
            Ok(client) => client,
            Err(e) => return ServerLlmStatus::unavailable(format!("{e:#}")),
        };
    let mut request = client.get(format!("{}/chat/completions", server_base(endpoint)));
    if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        request = request.bearer_auth(key);
    }
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) if voxctrl_pairing::is_certificate_mismatch(&e) => {
            return ServerLlmStatus::unavailable(voxctrl_pairing::CERTIFICATE_MISMATCH_HELP)
        }
        Err(e) if e.is_connect() => return ServerLlmStatus::unavailable("Could not connect to the server."),
        Err(e) if e.is_timeout() => return ServerLlmStatus::unavailable("The server did not answer in time."),
        Err(_) => return ServerLlmStatus::unavailable("The request to the server failed."),
    };
    let status = response.status().as_u16();
    let body = response.bytes().await.unwrap_or_default();
    interpret_server_status(status, &body)
}

fn interpret_server_status(status: u16, body: &[u8]) -> ServerLlmStatus {
    match status {
        200 => {
            let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let model = value.get("model").and_then(|m| m.as_str()).map(str::to_string);
            match value.get("available").and_then(|a| a.as_bool()) {
                Some(true) => ServerLlmStatus {
                    available: true,
                    message: match &model {
                        Some(model) => format!("This server does AI processing with {model}."),
                        None => "This server does AI processing.".into(),
                    },
                    model,
                },
                Some(false) => ServerLlmStatus::unavailable(
                    "AI processing is turned off on this server. On the server, set enabled = true, endpoint \
                     and model under [llm] in server.toml (or VOXCTRL_SERVER_LLM=true with \
                     VOXCTRL_SERVER_LLM_ENDPOINT and VOXCTRL_SERVER_LLM_MODEL) and restart it.",
                ),
                None => ServerLlmStatus::unavailable("This server does not offer AI processing."),
            }
        }
        401 | 403 => ServerLlmStatus::unavailable("The server did not accept the API key."),
        404 | 405 => ServerLlmStatus::unavailable(
            "This server does not offer AI processing. It needs voxctrl-server 0.9.16 or newer; other \
             speech servers do not have it.",
        ),
        other => ServerLlmStatus::unavailable(format!("The server answered HTTP {other}.")),
    }
}

#[cfg(test)]
mod tests {
    use super::{api_base, interpret_server_status, preset_system_prompt, server_base, OpenAiClient};
    use voxctrl_config::OpenAiMode;
    use voxctrl_config::{OpenAiConfig, RemoteOpenAiConfig};

    #[test]
    fn api_base_appends_v1() {
        assert_eq!(api_base("http://localhost:11434"), "http://localhost:11434/v1");
        assert_eq!(api_base("http://localhost:11434/"), "http://localhost:11434/v1");
    }

    #[test]
    fn api_base_preserves_existing_v1() {
        assert_eq!(api_base("https://api.openai.com/v1"), "https://api.openai.com/v1");
        assert_eq!(api_base("https://api.openai.com/v1/"), "https://api.openai.com/v1");
    }

    #[test]
    fn the_servers_base_follows_the_speech_endpoint_however_it_was_entered() {
        for endpoint in [
            "http://box:8787/v1",
            "http://box:8787/v1/",
            "http://box:8787",
            " http://box:8787/v1/audio/transcriptions ",
        ] {
            assert_eq!(server_base(endpoint), "http://box:8787/v1", "{endpoint}");
        }
    }

    #[test]
    fn going_through_the_server_uses_its_address_and_key_and_keeps_the_prompts() {
        let own = OpenAiConfig {
            enabled: true,
            endpoint: "http://localhost:11434".into(),
            api_key: Some("sk-own".into()),
            system_prompt: "Be formal.".into(),
            ..Default::default()
        };
        let server = RemoteOpenAiConfig {
            endpoint: "http://box:8787/v1".into(),
            api_key: Some("vx_key".into()),
            ..Default::default()
        };
        let client = OpenAiClient::via_server(own, &server);
        assert_eq!(client.config.endpoint, "http://box:8787/v1");
        assert_eq!(client.config.api_key.as_deref(), Some("vx_key"));
        assert_eq!(client.config.system_prompt, "Be formal.");
        assert!(client.refused.is_none());
    }

    #[tokio::test]
    async fn a_paired_server_given_a_plain_address_gets_nothing_sent() {
        let server = RemoteOpenAiConfig {
            endpoint: "http://box:8787/v1".into(),
            tls_fingerprint: Some("ab".repeat(32)),
            ..Default::default()
        };
        let client = OpenAiClient::via_server(OpenAiConfig { enabled: true, ..Default::default() }, &server);
        assert!(client.refused.as_deref().is_some_and(|r| r.contains("must start with https://")));
        // The text comes back as it was said.
        assert_eq!(client.process("hello there").await, "hello there");
    }

    #[test]
    fn server_status_is_read_from_what_the_server_says() {
        let on = interpret_server_status(200, br#"{"available": true, "model": "llama3.2:1b"}"#);
        assert!(on.available);
        assert_eq!(on.model.as_deref(), Some("llama3.2:1b"));
        assert_eq!(on.message, "This server does AI processing with llama3.2:1b.");

        let off = interpret_server_status(200, br#"{"available": false, "model": null}"#);
        assert!(!off.available);
        assert!(off.message.contains("VOXCTRL_SERVER_LLM=true"), "{}", off.message);

        assert!(interpret_server_status(401, b"").message.contains("API key"));
        // An older VoxCtrl server, or another speech server altogether.
        assert!(interpret_server_status(404, b"").message.contains("0.9.16 or newer"));
        assert!(interpret_server_status(405, b"").message.contains("0.9.16 or newer"));
        assert_eq!(interpret_server_status(500, b"").message, "The server answered HTTP 500.");
    }

    #[test]
    fn preset_custom_has_no_system_prompt() {
        assert_eq!(preset_system_prompt(&OpenAiMode::Custom), "");
        assert!(!preset_system_prompt(&OpenAiMode::Clean).is_empty());
        assert!(preset_system_prompt(&OpenAiMode::Formal).contains("formal"));
    }
}
