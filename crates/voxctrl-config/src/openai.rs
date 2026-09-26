//! The OpenAI-compatible text rewrite.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpenAiMode {
    #[default]
    Clean,
    Formal,
    Casual,
    Bullet,
    Concise,
    Custom,
}

impl OpenAiMode {
    /// Parse the snake_case name a mode is stored under (as in a hotkey
    /// binding's `openai_mode`), or `None` for a name no mode has.
    pub fn from_key(key: &str) -> Option<Self> {
        Some(match key {
            "clean" => Self::Clean,
            "formal" => Self::Formal,
            "casual" => Self::Casual,
            "bullet" => Self::Bullet,
            "concise" => Self::Concise,
            "custom" => Self::Custom,
            _ => return None,
        })
    }
}

fn default_system_prompt() -> String {
    "Fix grammar and punctuation only. Return only the corrected text, no commentary.".into()
}

fn default_user_prompt() -> String {
    "{text}".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiConfig {
    pub enabled: bool,
    pub model: String,
    /// Preset that last populated the system prompt. Kept for UI convenience and
    /// backward compatibility; generation is driven by `system_prompt`/`user_prompt`.
    #[serde(default)]
    pub mode: OpenAiMode,
    /// Legacy single-prompt template (mode == Custom). Migrated into `user_prompt`.
    #[serde(default)]
    pub custom_prompt: Option<String>,
    /// System message sent to the model. Empty = no system message.
    #[serde(default = "default_system_prompt")]
    pub system_prompt: String,
    /// User message template. Must contain "{text}", which is replaced with the
    /// dictated text before being sent to the model.
    #[serde(default = "default_user_prompt")]
    pub user_prompt: String,
    /// Base URL of the OpenAI-compatible API server (a local server or a remote
    /// provider). May optionally include a `/v1` suffix.
    pub endpoint: String,
    /// Optional API key sent as a `Bearer` token. Required by most remote
    /// providers; usually unnecessary for a local server.
    #[serde(default)]
    pub api_key: Option<String>,
    pub timeout_secs: u64,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "llama3.2:1b".into(),
            mode: OpenAiMode::Clean,
            custom_prompt: None,
            system_prompt: default_system_prompt(),
            user_prompt: default_user_prompt(),
            endpoint: "http://localhost:11434".into(),
            api_key: None,
            timeout_secs: 30,
        }
    }
}
