use std::path::PathBuf;

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

// ── Engine sub-configs ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhisperCppConfig {
    /// Directory containing GGUF model files. Empty = platform default.
    pub model_dir: String,
    /// Model size name: "tiny", "base", "small", "medium", "large-v3", etc.
    pub model_size: String,
    /// "auto" | "cuda" | "vulkan" | "cpu"
    pub device: String,
    /// 0 = auto-detect (half of logical cores)
    pub threads: u32,
}

impl Default for WhisperCppConfig {
    fn default() -> Self {
        Self {
            model_dir: String::new(),
            model_size: "large-v3".into(),
            device: "auto".into(),
            threads: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
#[serde(rename_all = "kebab-case")]
pub enum BackendChoice {
    Auto,
    WhisperCpp,
    Moonshine,
}

impl Default for BackendChoice {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum InferenceMode {
    Balanced,
    Aggressive,
}

impl Default for InferenceMode {
    fn default() -> Self {
        Self::Balanced
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EngineConfig {
    pub backend: BackendChoice,
    pub inference_mode: InferenceMode,
    pub whisper_cpp: WhisperCppConfig,
    pub moonshine: MoonshineConfig,
}

// ── Audio ─────────────────────────────────────────────────────────────────────

fn default_gain() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub vad_threshold: f32,
    pub min_silence_duration_ms: u32,
    /// None = use default system device
    pub input_device_index: Option<u32>,
    /// Saved evdev device path, e.g. "/dev/input/event4" (Linux only)
    pub evdev_device: Option<String>,
    pub noise_suppression: bool,
    /// Linear gain multiplier applied before sending to inference (1.0 = unity)
    #[serde(default = "default_gain")]
    pub gain: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            vad_threshold: 0.5,
            min_silence_duration_ms: 500,
            input_device_index: None,
            evdev_device: None,
            noise_suppression: false,
            gain: 1.0,
        }
    }
}

// ── UI ────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayStyle {
    VoiceCard,
    Waveform,
    Pulse,
    BlueWave,
    None,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self::VoiceCard
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub show_overlay: bool,
    pub overlay_style: OverlayStyle,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_overlay: true,
            overlay_style: OverlayStyle::VoiceCard,
        }
    }
}

// ── Features ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    pub remove_fillers: bool,
    pub custom_vocabulary: Vec<String>,
    pub spoken_punctuation: bool,
    pub auto_format_lists: bool,
    pub quiet_mode: bool,
    pub show_notification: bool,
    /// Map of trigger → expansion, e.g. {"addr" → "123 Main St"}
    pub snippets: std::collections::HashMap<String, String>,
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            remove_fillers: true,
            custom_vocabulary: Vec::new(),
            spoken_punctuation: true,
            auto_format_lists: true,
            quiet_mode: false,
            show_notification: false,
            snippets: std::collections::HashMap::new(),
        }
    }
}

// ── Ollama ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OllamaMode {
    Clean,
    Formal,
    Casual,
    Bullet,
    Concise,
    Custom,
}

impl Default for OllamaMode {
    fn default() -> Self {
        Self::Clean
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaConfig {
    pub enabled: bool,
    pub model: String,
    pub mode: OllamaMode,
    /// Used when mode == Custom. "{text}" is substituted.
    pub custom_prompt: Option<String>,
    pub endpoint: String,
    pub timeout_secs: u64,
}

impl Default for OllamaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: "llama3.2:1b".into(),
            mode: OllamaMode::Clean,
            custom_prompt: None,
            endpoint: "http://localhost:11434".into(),
            timeout_secs: 8,
        }
    }
}

// ── TTS ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsEngine {
    Piper,
    Espeak,
}

impl Default for TtsEngine {
    fn default() -> Self {
        Self::Piper
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub enabled: bool,
    pub engine: TtsEngine,
    /// Voice name, e.g. "en-us-lessac-medium"
    pub voice: String,
    /// Key(s) that stop TTS playback, e.g. ["KEY_ESCAPE"]
    pub stop_key: Vec<String>,
    pub response_overlay: bool,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            engine: TtsEngine::Piper,
            voice: "en-us-lessac-medium".into(),
            stop_key: vec!["KEY_ESCAPE".into()],
            response_overlay: true,
        }
    }
}

// ── MCP ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub server_enabled: bool,
    pub record_timeout: f64,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            server_enabled: false,
            record_timeout: 15.0,
        }
    }
}

// ── AT-SPI2 ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AtspiConfig {
    /// Use AT-SPI2 for text insertion when available
    pub injection: bool,
    /// Feed surrounding text to Whisper as initial prompt
    pub context_prompt: bool,
    /// Automatically switch to code mode in terminals/IDEs
    pub auto_code_mode: bool,
}

impl Default for AtspiConfig {
    fn default() -> Self {
        Self {
            injection: true,
            context_prompt: true,
            auto_code_mode: true,
        }
    }
}

// ── Root config ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub engine: EngineConfig,
    pub audio: AudioConfig,
    pub ui: UiConfig,
    pub features: FeaturesConfig,
    pub ollama: OllamaConfig,
    pub tts: TtsConfig,
    pub mcp: McpConfig,
    pub atspi: AtspiConfig,
}

// ── Config manager ────────────────────────────────────────────────────────────

pub struct Config {
    pub data: AppConfig,
    path: PathBuf,
}

impl Config {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctl")
            .join("config.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        let data = if path.exists() {
            match std::fs::read_to_string(&path)
                .map_err(ConfigError::Io)
                .and_then(|s| serde_json::from_str::<AppConfig>(&s).map_err(ConfigError::Json))
            {
                Ok(cfg) => cfg,
                Err(e) => {
                    tracing::warn!("Failed to load config, using defaults: {e}");
                    AppConfig::default()
                }
            }
        } else {
            AppConfig::default()
        };
        Self { data, path }
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn reload(&mut self) {
        *self = Self::load();
    }
}

impl Default for Config {
    fn default() -> Self {
        Self::load()
    }
}

// ── Validation ────────────────────────────────────────────────────────────────

static VALID_MODEL_SIZES: &[&str] = &[
    "tiny", "tiny.en", "base", "base.en", "small", "small.en",
    "medium", "medium.en", "large-v2", "large-v3", "large-v3-turbo",
];

pub fn validate(cfg: &AppConfig) -> Vec<String> {
    let mut errors = Vec::new();

    if !VALID_MODEL_SIZES.contains(&cfg.engine.whisper_cpp.model_size.as_str())
        && !cfg.engine.whisper_cpp.model_size.ends_with(".bin")
        && !std::path::Path::new(&cfg.engine.whisper_cpp.model_size).is_absolute()
    {
        errors.push(format!(
            "Unknown whisper_cpp model_size '{}'. Valid: {:?}",
            cfg.engine.whisper_cpp.model_size, VALID_MODEL_SIZES
        ));
    }

    if !["auto", "cuda", "vulkan", "cpu"]
        .contains(&cfg.engine.whisper_cpp.device.as_str())
    {
        errors.push(format!(
            "Invalid whisper_cpp device '{}'. Use: auto, cuda, vulkan, cpu",
            cfg.engine.whisper_cpp.device
        ));
    }

    if cfg.audio.vad_threshold < 0.0 || cfg.audio.vad_threshold > 1.0 {
        errors.push(format!(
            "vad_threshold {} out of range [0.0, 1.0]",
            cfg.audio.vad_threshold
        ));
    }

    errors
}
