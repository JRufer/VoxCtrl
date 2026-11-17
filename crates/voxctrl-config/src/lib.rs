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

mod audio;
mod engine;
mod features;
mod history;
mod mcp;
mod migrate;
mod openai;
mod paths;
mod store;
mod tts;
mod ui;
mod updates;
mod validate;

pub use audio::*;
pub use engine::*;
pub use features::*;
pub use history::*;
pub use mcp::*;
use migrate::*;
pub use migrate::{canonical_key_name, canonicalize_key_names};
pub use openai::*;
pub use paths::*;
pub use store::*;
pub use tts::*;
pub use ui::*;
pub use updates::*;
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
    /// Absent from configs written before history existed.
    #[serde(default)]
    pub history: HistoryConfig,
    /// Absent from configs written before automatic update checks became an
    /// opt-in. See `updates.rs` for why an older `updates` section's keys are
    /// not read.
    #[serde(default)]
    pub updates: UpdateConfig,
}

#[cfg(test)]
mod tests;
