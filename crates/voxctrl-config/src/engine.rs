//! Speech-engine settings: the backend choice and each backend's own section.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WhisperCppConfig {
    /// Directory containing GGUF model files. Empty = platform default.
    pub model_dir: String,
    /// Model size name: "tiny", "base", "small", "medium", "large-v3", etc.
    pub model_size: String,
    /// "auto" | "cuda" | "vulkan" | "cpu"
    pub device: String,
    /// 0 = auto-detect (half of logical cores)
    pub threads: u32,
    /// BCP-47 language code, e.g. "en", or "auto" to let whisper.cpp detect it.
    #[serde(default = "default_whisper_cpp_language")]
    pub language: String,
}

fn default_whisper_cpp_language() -> String {
    "auto".into()
}

impl Default for WhisperCppConfig {
    fn default() -> Self {
        Self {
            model_dir: String::new(),
            // "tiny" (~75MB) is small enough to auto-download silently at
            // first launch (see src-tauri/src/lib.rs startup hook) so the app
            // transcribes out of the box with no manual download step. Users
            // who want more accuracy can pick a larger model in Settings.
            model_size: "tiny".into(),
            device: "auto".into(),
            threads: 0,
            language: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MoonshineConfig {
    /// "base" or "tiny"
    pub model_size: String,
    /// BCP-47 language code, e.g. "en"
    pub language: String,
}

impl Default for MoonshineConfig {
    fn default() -> Self {
        Self {
            model_size: "base".into(),
            language: "en".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParakeetConfig {
    pub model_size: String,
    pub language: String,
}

impl Default for ParakeetConfig {
    fn default() -> Self {
        Self {
            model_size: "tdt-0.6b-v3".into(),
            language: "auto".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteOpenAiConfig {
    /// Remote OpenAI-compatible endpoint URL, e.g. "http://localhost:8000/v1"
    pub endpoint: String,
    /// Optional API key for Bearer authorization
    pub api_key: Option<String>,
    /// Model identifier, e.g. "whisper-1", "whisper-large-v3"
    pub model: String,
    /// Optional language code, e.g. "en" (empty string = auto-detect)
    pub language: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for RemoteOpenAiConfig {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:8000/v1".into(),
            api_key: None,
            model: "whisper-1".into(),
            language: "".into(),
            timeout_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BackendChoice {
    /// `alias = "auto"` migrates configs written when the backend could be
    /// left unset: auto-selection always resolved to whisper.cpp anyway, so
    /// those installs keep the backend they were already running.
    #[serde(alias = "auto")]
    #[default]
    WhisperCpp,
    Moonshine,
    Parakeet,
    #[serde(rename = "remote-openai", alias = "remote-open-ai", alias = "remote_openai", alias = "openai-compatible", alias = "remote")]
    RemoteOpenAi,
}

fn default_s1_mini_styling() -> String {
    "semi-formal".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct S1MiniConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_s1_mini_styling")]
    pub styling: String,
}

impl Default for S1MiniConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            styling: default_s1_mini_styling(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EngineConfig {
    #[serde(default)]
    pub backend: BackendChoice,
    #[serde(default)]
    pub whisper_cpp: WhisperCppConfig,
    #[serde(default)]
    pub moonshine: MoonshineConfig,
    #[serde(default)]
    pub parakeet: ParakeetConfig,
    #[serde(default)]
    pub remote_openai: RemoteOpenAiConfig,
    #[serde(default)]
    pub s1_mini: S1MiniConfig,
}
