
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Validation error: {0}")]
    Validation(String),
}

mod engine;
mod audio;
mod ui;
mod features;
mod openai;
mod tts;
mod mcp;
mod migrate;
mod store;
mod paths;
mod validate;

pub use engine::*;
pub use audio::*;
pub use ui::*;
pub use features::*;
pub use openai::*;
pub use tts::*;
pub use mcp::*;
use migrate::*;
pub use migrate::{canonical_key_name, canonicalize_key_names};
pub use store::*;
pub use paths::*;
pub use validate::*;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub engine: EngineConfig,
    pub audio: AudioConfig,
    pub ui: UiConfig,
    pub features: FeaturesConfig,
    /// `alias = "ollama"` keeps configs written before the OpenAI-API rename loading.
    #[serde(alias = "ollama")]
    pub openai: OpenAiConfig,
    pub tts: TtsConfig,
    pub mcp: McpConfig,
}

#[cfg(test)]
mod tests;
