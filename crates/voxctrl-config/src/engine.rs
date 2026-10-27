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
    /// "cpu" | "auto" (GPU when the build has an ONNX Runtime GPU provider).
    #[serde(default = "default_onnx_device")]
    pub device: String,
}

/// Moonshine and Parakeet run their graphs part on the GPU and part on the CPU
/// and bounce data across the bus on every decoding step, which measured 5–7×
/// slower than the CPU on an RTX 4090 (see `voxctrl_inference::bench`). So the
/// GPU is opt-in, and the Engine tab's benchmark says whether it pays off on a
/// given machine.
fn default_onnx_device() -> String {
    "cpu".into()
}

impl Default for MoonshineConfig {
    fn default() -> Self {
        Self {
            model_size: "base".into(),
            language: "en".into(),
            device: default_onnx_device(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParakeetConfig {
    pub model_size: String,
    pub language: String,
    /// "auto" (GPU when the build has an ONNX Runtime GPU provider) | "cpu".
    #[serde(default = "default_onnx_device")]
    pub device: String,
}

impl Default for ParakeetConfig {
    fn default() -> Self {
        Self {
            model_size: "tdt-0.6b-v3".into(),
            language: "auto".into(),
            device: default_onnx_device(),
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
    /// Offload cleanup to the GPU when the sidecar was built with Vulkan.
    /// `false` forces the CPU.
    #[serde(default = "default_true")]
    pub gpu: bool,
    /// A different styling per app: the first rule whose `app` matches the
    /// app focused when dictation started wins, otherwise `styling` applies.
    #[serde(default)]
    pub app_styles: Vec<AppStyleRule>,
}

/// "Use this cleanup styling when dictating into this app."
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppStyleRule {
    /// Matched case-insensitively against the focused app's name (its window
    /// class on Linux, its program name on Windows); a rule matches when this
    /// text appears anywhere in it, so `slack` matches `Slack` and
    /// `com.slack.Slack`.
    pub app: String,
    pub styling: String,
}

impl S1MiniConfig {
    /// The styling for a dictation into `app` (the focused app's name, if it
    /// could be found): the first matching rule's, otherwise the default.
    pub fn styling_for(&self, app: Option<&str>) -> &str {
        let Some(app) = app.map(str::trim).filter(|a| !a.is_empty()) else {
            return &self.styling;
        };
        let app = app.to_lowercase();
        self.app_styles
            .iter()
            .find(|rule| {
                let pattern = rule.app.trim().to_lowercase();
                !pattern.is_empty() && !rule.styling.trim().is_empty() && app.contains(&pattern)
            })
            .map_or(&self.styling, |rule| &rule.styling)
    }
}

fn default_true() -> bool {
    true
}

impl Default for S1MiniConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            styling: default_s1_mini_styling(),
            gpu: true,
            app_styles: Vec::new(),
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

#[cfg(test)]
mod app_style_tests {
    use super::*;

    fn cfg(rules: &[(&str, &str)]) -> S1MiniConfig {
        S1MiniConfig {
            app_styles: rules
                .iter()
                .map(|(app, styling)| AppStyleRule { app: app.to_string(), styling: styling.to_string() })
                .collect(),
            ..S1MiniConfig::default()
        }
    }

    #[test]
    fn no_app_or_no_rule_uses_the_default() {
        let c = cfg(&[("slack", "casual")]);
        assert_eq!(c.styling_for(None), "semi-formal");
        assert_eq!(c.styling_for(Some("")), "semi-formal");
        assert_eq!(c.styling_for(Some("firefox")), "semi-formal");
    }

    #[test]
    fn rules_match_case_insensitively_anywhere_in_the_name() {
        let c = cfg(&[("slack", "casual"), ("Thunderbird", "formal")]);
        assert_eq!(c.styling_for(Some("Slack")), "casual");
        assert_eq!(c.styling_for(Some("com.slack.Slack")), "casual");
        assert_eq!(c.styling_for(Some("thunderbird")), "formal");
    }

    #[test]
    fn the_first_matching_rule_wins() {
        let c = cfg(&[("code", "verbatim"), ("code-oss", "concise")]);
        assert_eq!(c.styling_for(Some("code-oss")), "verbatim");
    }

    #[test]
    fn blank_rules_are_ignored() {
        let c = cfg(&[("", "casual"), ("slack", " "), ("slack", "casual")]);
        assert_eq!(c.styling_for(Some("firefox")), "semi-formal");
        assert_eq!(c.styling_for(Some("slack")), "casual");
    }

    #[test]
    fn older_configs_without_rules_still_load() {
        let c: S1MiniConfig = serde_json::from_str(r#"{"enabled": true, "styling": "formal"}"#).unwrap();
        assert!(c.app_styles.is_empty());
        assert_eq!(c.styling_for(Some("slack")), "formal");
    }
}
