pub mod backend;
pub mod postprocess;
pub mod whisper_cpp;

use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use tracing::{error, info, warn};
use voxctrl_config::{AppConfig, BackendChoice};

use backend::{TranscribeRequest, TranscriptionBackend};
use postprocess::{run_pipeline, PostProcessConfig, is_silence_hallucination};
use whisper_cpp::WhisperCppBackend;

// ── Audio chunk type (must match voxctrl-audio) ────────────────────────────────

pub type AudioChunk = Vec<f32>;

// ── Inference request ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// Accumulated audio samples (16 kHz, mono, f32)
    pub audio: Vec<f32>,
    /// Target id (used to look up per-target processing overrides)
    pub target_id: String,
    /// Hotkey binding ID (if triggered by a hotkey)
    pub binding_id: Option<String>,
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
        let config_path = voxctrl_config::Config::config_path();
        let app_config = if config_path.exists() {
            std::fs::read_to_string(&config_path)
                .ok()
                .and_then(|s| serde_json::from_str::<voxctrl_config::AppConfig>(&s).ok())
                .unwrap_or_else(|| (*self.config).clone())
        } else {
            (*self.config).clone()
        };

        // ── Noise Gate (VAD) ──────────────────────────────────────────────────
        // Compute RMS energy of the entire audio request to implement a robust noise gate.
        let rms = {
            let sum_sq: f32 = req.audio.iter().map(|&s| s * s).sum();
            (sum_sq / req.audio.len() as f32).sqrt()
        };

        // Map vad_threshold (0.0 - 1.0) to physical RMS threshold.
        // Invert so that 1.0 represents MAXIMUM sensitivity (completely open gate / 0.0 RMS threshold).
        // 0.0 represents MINIMUM sensitivity (highest gate / 0.006 RMS threshold).
        // A default slider value of 0.5 maps to 0.003 RMS, which easily lets speech through while filtering silence.
        let rms_threshold = (1.0 - app_config.audio.vad_threshold) * 0.006;

        if rms < rms_threshold {
            info!(
                "Audio skipped by noise gate: RMS is {:.5} (threshold is {:.5}, vad_threshold={:.2})",
                rms,
                rms_threshold,
                app_config.audio.vad_threshold
            );
            return Ok(InferenceOutput {
                text: String::new(),
                target_id: req.target_id,
                raw_text: String::new(),
                inference_ms: 0,
                language: "en".into(),
            });
        }

        let dir = voxctrl_routing::config_dir();
        let targets = voxctrl_routing::load_targets(&dir).unwrap_or_default();
        let target_ids: Vec<&str> = req.target_id.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let first_target_id = target_ids.first().copied().unwrap_or("default");
        let target = targets.iter().find(|t| t.id == first_target_id);

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

        let post_cfg = self.build_post_config_with_app_config(&req.target_id, &app_config, &targets);
        let mut processed = run_pipeline(&raw_text, &post_cfg);

        // ── Silence Hallucination Filter ──────────────────────────────────────
        // If Whisper returned a known silence hallucination (like "Thank you"), check if the audio energy
        // was extremely low (e.g. below 0.003 RMS, which is absolute background room silence).
        // This ensures the user can still say a genuine, spoken "Thank you" (which has much higher energy),
        // while perfectly discarding silence-induced hallucinations when sensitivity is set high.
        if !processed.is_empty() && is_silence_hallucination(&processed) && rms < 0.003 {
            info!("Discarded silence hallucination '{}' (audio RMS: {:.5})", processed, rms);
            processed = String::new();
        }

        // ── Hotkey-Specific Ollama Post-Processing ────────────────────────────
        let bindings = voxctrl_routing::load_bindings(&dir).unwrap_or_default();
        let binding = req.binding_id.as_ref().and_then(|bid| bindings.iter().find(|b| &b.id == bid));

        let binding_wants_ollama = binding
            .and_then(|b| b.ollama_enabled)
            .unwrap_or(false);

        if binding_wants_ollama && !processed.is_empty() {
            let mut ollama_cfg = app_config.ollama.clone();
            ollama_cfg.enabled = true;

            if let Some(ref b) = binding {
                if let Some(ref model) = b.ollama_model {
                    if !model.is_empty() {
                        ollama_cfg.model = model.clone();
                    }
                }
                if let Some(ref mode_str) = b.ollama_mode {
                    ollama_cfg.mode = match mode_str.as_str() {
                        "clean" => voxctrl_config::OllamaMode::Clean,
                        "formal" => voxctrl_config::OllamaMode::Formal,
                        "casual" => voxctrl_config::OllamaMode::Casual,
                        "bullet" => voxctrl_config::OllamaMode::Bullet,
                        "concise" => voxctrl_config::OllamaMode::Concise,
                        "custom" => voxctrl_config::OllamaMode::Custom,
                        _ => voxctrl_config::OllamaMode::Clean,
                    };
                }
                if let Some(ref prompt) = b.ollama_prompt {
                    if !prompt.is_empty() {
                        ollama_cfg.custom_prompt = Some(prompt.clone());
                        ollama_cfg.mode = voxctrl_config::OllamaMode::Custom;
                    }
                }
            }

            let client = voxctrl_llm::OllamaClient::new(ollama_cfg);
            let processed_res = match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    let c = client.clone();
                    let text = processed.clone();
                    std::thread::spawn(move || {
                        handle.block_on(async { c.process(&text).await })
                    }).join().unwrap_or(processed)
                }
                Err(_) => {
                    if let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() {
                        rt.block_on(async { client.process(&processed).await })
                    } else {
                        processed
                    }
                }
            };
            processed = processed_res;
        }

        Ok(InferenceOutput {
            text: processed,
            target_id: req.target_id,
            raw_text,
            inference_ms: result.inference_ms,
            language: result.language,
        })
    }

    fn build_post_config_with_app_config(&self, target_id: &str, app_config: &voxctrl_config::AppConfig, targets: &[voxctrl_routing::OutputTarget]) -> PostProcessConfig {
        let target_ids: Vec<&str> = target_id.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
        let first_target_id = target_ids.first().copied().unwrap_or("default");
        let target = targets.iter().find(|t| t.id == first_target_id);

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
        .name("voxctrl-inference".into())
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
                        let _ = tx.send(output);
                    }
                    Err(e) => {
                        error!("Inference error: {:?}", e);
                        let _ = tx.send(InferenceOutput {
                            text: "".to_string(),
                            target_id: "".to_string(),
                            raw_text: "".to_string(),
                            inference_ms: 0,
                            language: "".to_string(),
                        });
                    }
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
