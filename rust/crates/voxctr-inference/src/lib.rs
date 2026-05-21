pub mod backend;
pub mod postprocess;
pub mod whisper_cpp;

use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use tracing::{error, info, warn};
use voxctr_config::{AppConfig, BackendChoice};

use backend::{TranscribeRequest, TranscriptionBackend};
use postprocess::{run_pipeline, PostProcessConfig};
use whisper_cpp::WhisperCppBackend;

// ── Audio chunk type (must match voxctr-audio) ────────────────────────────────

pub type AudioChunk = Vec<f32>;

// ── Inference request ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// Accumulated audio samples (16 kHz, mono, f32)
    pub audio: Vec<f32>,
    /// Target id (used to look up per-target processing overrides)
    pub target_id: String,
    /// AT-SPI2 surrounding text for initial prompt, if available
    pub context_text: Option<String>,
}

/// Final output after transcription + post-processing.
#[derive(Debug, Clone)]
pub struct InferenceOutput {
    pub text: String,
    pub target_id: String,
    pub raw_text: String,
    pub inference_ms: u32,
    pub language: String,
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct InferenceEngine {
    config: Arc<AppConfig>,
    backend: Box<dyn TranscriptionBackend>,
}

impl InferenceEngine {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let backend = build_backend(&config);
        Self { config, backend }
    }

    /// Load the selected backend model. Blocks until ready.
    pub fn load(&mut self) -> Result<()> {
        self.backend.load()
    }

    pub fn unload(&mut self) {
        self.backend.unload();
    }

    /// Transcribe and post-process. Returns the final text.
    pub fn process(&self, req: InferenceRequest) -> Result<InferenceOutput> {
        if req.audio.is_empty() {
            return Ok(InferenceOutput {
                text: String::new(),
                target_id: req.target_id,
                raw_text: String::new(),
                inference_ms: 0,
                language: "en".into(),
            });
        }

        let language = if self.config.engine.whisper_cpp.device == "auto" {
            None
        } else {
            Some(self.config.engine.moonshine.language.clone())
        };

        // Fetch custom vocabulary from config and target-specific initial_prompt
        let config_path = voxctr_config::Config::config_path();
        let app_config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<voxctr_config::AppConfig>(&s).ok())
                .unwrap_or_else(|| (*self.config).clone())
        } else {
            (*self.config).clone()
        };

        let dir = voxctr_routing::config_dir();
        let targets = voxctr_routing::load_targets(&dir).unwrap_or_default();
        let target = targets.iter().find(|t| t.id == req.target_id);

        let mut merged_prompt = String::new();

        // 1. Target's initial prompt if defined
        if let Some(target_prompt) = target.and_then(|t| t.initial_prompt.as_ref()) {
            if !target_prompt.trim().is_empty() {
                merged_prompt.push_str(target_prompt.trim());
                merged_prompt.push_str(". ");
            }
        }

        // 2. Custom vocabulary words from features config
        if !app_config.features.custom_vocabulary.is_empty() {
            // Append as: "Vocabulary: word1, word2, word3..."
            merged_prompt.push_str("Vocabulary: ");
            merged_prompt.push_str(&app_config.features.custom_vocabulary.join(", "));
            merged_prompt.push_str(". ");
        }

        // 3. Fallback context text if available
        if let Some(ref context) = req.context_text {
            if !context.trim().is_empty() {
                merged_prompt.push_str(context.trim());
                merged_prompt.push_str(". ");
            }
        }

        let initial_prompt = if merged_prompt.trim().is_empty() {
            None
        } else {
            Some(merged_prompt.trim().to_string())
        };

        let t_req = TranscribeRequest {
            audio: req.audio,
            language,
            word_timestamps: false,
            initial_prompt,
        };

        let result = self.backend.transcribe(&t_req)?;
        let raw_text = result.text.clone();

        let post_cfg = self.build_post_config_with_app_config(&req.target_id, &app_config);
        let processed = run_pipeline(&raw_text, &post_cfg);

        Ok(InferenceOutput {
            text: processed,
            target_id: req.target_id,
            raw_text,
            inference_ms: result.inference_ms,
            language: result.language,
        })
    }

    fn build_post_config(&self, target_id: &str) -> PostProcessConfig {
        // Load the config from the standard file path dynamically so edits are updated instantly
        let config_path = voxctr_config::Config::config_path();
        let app_config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<voxctr_config::AppConfig>(&s).ok())
                .unwrap_or_else(|| (*self.config).clone())
        } else {
            (*self.config).clone()
        };

        self.build_post_config_with_app_config(target_id, &app_config)
    }

    fn build_post_config_with_app_config(&self, target_id: &str, app_config: &voxctr_config::AppConfig) -> PostProcessConfig {
        // Load routing targets from the routing directory
        let dir = voxctr_routing::config_dir();
        let targets = voxctr_routing::load_targets(&dir).unwrap_or_default();
        let target = targets.iter().find(|t| t.id == target_id);

        let remove_fillers = target
            .and_then(|t| t.processing.remove_fillers)
            .unwrap_or(app_config.features.remove_fillers);

        let spoken_punctuation = target
            .and_then(|t| t.processing.spoken_punctuation)
            .unwrap_or(app_config.features.spoken_punctuation);

        let auto_format_lists = target
            .and_then(|t| t.processing.auto_format_lists)
            .unwrap_or(app_config.features.auto_format_lists);

        let apply_snippets = target
            .and_then(|t| t.processing.apply_snippets)
            .unwrap_or(true); // default to true

        let code_mode = target
            .and_then(|t| t.processing.code_mode)
            .unwrap_or(false);

        PostProcessConfig {
            remove_fillers,
            spoken_punctuation,
            auto_format_lists,
            apply_snippets: apply_snippets && !app_config.features.snippets.is_empty(),
            snippets: app_config.features.snippets.clone(),
            code_mode,
            custom_vocabulary: app_config.features.custom_vocabulary.clone(),
        }
    }
}

// ── Backend selection ─────────────────────────────────────────────────────────

fn build_backend(config: &AppConfig) -> Box<dyn TranscriptionBackend> {
    match config.engine.backend {
        BackendChoice::WhisperCpp => {
            Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
        }
        BackendChoice::Moonshine => {
            // Moonshine feature not compiled — fall back to whisper-cpp
            warn!("Moonshine backend selected but not compiled; using whisper-cpp");
            Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
        }
        BackendChoice::Auto => {
            let selected = auto_select(config);
            info!("Auto-selected backend: {}", selected.name());
            selected
        }
    }
}

fn auto_select(config: &AppConfig) -> Box<dyn TranscriptionBackend> {
    // GPU detection
    let has_nvidia = detect_nvidia();
    let has_vulkan = detect_vulkan();

    if has_nvidia || has_vulkan {
        info!(
            nvidia = has_nvidia,
            vulkan = has_vulkan,
            "GPU detected; using whisper-cpp with GPU acceleration"
        );
    } else {
        info!("No GPU detected; using whisper-cpp CPU");
    }

    Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
}

fn detect_nvidia() -> bool {
    // 1. Try standard nvidia-smi command
    if let Ok(output) = std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
    {
        if output.status.success() {
            return true;
        }
    }

    // 2. Check if Nvidia driver proc file exists (Linux proprietary driver)
    if std::path::Path::new("/proc/driver/nvidia/version").exists() {
        return true;
    }

    // 3. Check for Nvidia device nodes in /dev
    if std::path::Path::new("/dev/nvidia0").exists() || std::path::Path::new("/dev/nvidiactl").exists() {
        return true;
    }

    false
}

fn detect_vulkan() -> bool {
    // 1. Try vulkaninfo
    if let Ok(output) = std::process::Command::new("vulkaninfo")
        .arg("--summary")
        .output()
    {
        if output.status.success() {
            return true;
        }
    }

    // 2. Check for Vulkan ICD loader configs
    if std::path::Path::new("/usr/share/vulkan/icd.d").exists() || std::path::Path::new("/etc/vulkan/icd.d").exists() {
        return true;
    }

    false
}

// ── Threaded worker ───────────────────────────────────────────────────────────

/// Run the inference engine on a dedicated OS thread.
/// Receives `InferenceRequest` from `rx`, sends `InferenceOutput` to `tx`.
pub fn run_worker(
    config: Arc<AppConfig>,
    rx: Receiver<InferenceRequest>,
    tx: Sender<InferenceOutput>,
) {
    std::thread::Builder::new()
        .name("voxctr-inference".into())
        .spawn(move || {
            let mut engine = InferenceEngine::new(config);
            if let Err(e) = engine.load() {
                error!("Failed to load inference backend: {:?}", e);
                return;
            }
            info!("Inference engine ready");

            while let Ok(req) = rx.recv() {
                match engine.process(req) {
                    Ok(output) => {
                        if !output.text.is_empty() {
                            let _ = tx.send(output);
                        }
                    }
                    Err(e) => error!("Inference error: {:?}", e),
                }
            }
        })
        .expect("failed to spawn inference thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_nvidia_does_not_panic() {
        // Just verify execution compiles and runs
        let _ = detect_nvidia();
    }

    #[test]
    fn test_detect_vulkan_does_not_panic() {
        // Just verify execution compiles and runs
        let _ = detect_vulkan();
    }

    #[test]
    fn test_auto_select_backend() {
        let mut cfg = AppConfig::default();
        cfg.engine.whisper_cpp.device = "auto".to_string();
        let backend = auto_select(&cfg);
        assert_eq!(backend.name(), "whisper-cpp");
    }
}
