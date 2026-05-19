pub mod backend;
pub mod postprocess;
pub mod whisper_cpp;

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use tracing::{debug, error, info, warn};
use voxctr_config::{AppConfig, BackendChoice};

use backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};
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

        let t_req = TranscribeRequest {
            audio: req.audio,
            language,
            word_timestamps: false,
            initial_prompt: req.context_text,
        };

        let result = self.backend.transcribe(&t_req)?;
        let raw_text = result.text.clone();

        let post_cfg = self.build_post_config(&req.target_id);
        let processed = run_pipeline(&raw_text, &post_cfg);

        Ok(InferenceOutput {
            text: processed,
            target_id: req.target_id,
            raw_text,
            inference_ms: result.inference_ms,
            language: result.language,
        })
    }

    fn build_post_config(&self, _target_id: &str) -> PostProcessConfig {
        // TODO: merge per-target overrides from routing config
        PostProcessConfig {
            remove_fillers: self.config.features.remove_fillers,
            spoken_punctuation: self.config.features.spoken_punctuation,
            auto_format_lists: self.config.features.auto_format_lists,
            apply_snippets: !self.config.features.snippets.is_empty(),
            snippets: self.config.features.snippets.clone(),
            code_mode: false,
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
    std::process::Command::new("nvidia-smi")
        .arg("--query-gpu=name")
        .arg("--format=csv,noheader")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn detect_vulkan() -> bool {
    std::process::Command::new("vulkaninfo")
        .arg("--summary")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
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
                error!("Failed to load inference backend: {e}");
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
                    Err(e) => error!("Inference error: {e}"),
                }
            }
        })
        .expect("failed to spawn inference thread");
}
