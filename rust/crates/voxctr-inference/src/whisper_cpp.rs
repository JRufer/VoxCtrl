use std::{
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{bail, Context, Result};
use tracing::info;
use voxctr_config::WhisperCppConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

// ── GGUF model resolution ─────────────────────────────────────────────────────

static GGUF_MAP: &[(&str, &[&str])] = &[
    ("tiny",           &["ggml-tiny-q5_1.bin",           "ggml-tiny.bin"]),
    ("tiny.en",        &["ggml-tiny.en-q5_1.bin",        "ggml-tiny.en.bin"]),
    ("base",           &["ggml-base-q5_1.bin",           "ggml-base.bin"]),
    ("base.en",        &["ggml-base.en-q5_1.bin",        "ggml-base.en.bin"]),
    ("small",          &["ggml-small-q5_1.bin",          "ggml-small.bin"]),
    ("small.en",       &["ggml-small.en-q5_1.bin",       "ggml-small.en.bin"]),
    ("medium",         &["ggml-medium-q5_0.bin",         "ggml-medium.bin"]),
    ("medium.en",      &["ggml-medium.en-q5_0.bin",      "ggml-medium.en.bin"]),
    ("large-v2",       &["ggml-large-v2-q5_0.bin",       "ggml-large-v2.bin"]),
    ("large-v3",       &["ggml-large-v3-q5_0.bin",       "ggml-large-v3.bin"]),
    ("large-v3-turbo", &["ggml-large-v3-turbo-q5_0.bin", "ggml-large-v3-turbo.bin"]),
];

const GGUF_BASE_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";

// ── Backend ───────────────────────────────────────────────────────────────────

pub struct WhisperCppBackend {
    cfg: WhisperCppConfig,
    model_path: Option<PathBuf>,
    loaded: bool,

    /// whisper-rs in-process model
    inner: Option<whisper_rs::WhisperContext>,
}

impl WhisperCppBackend {
    pub fn new(cfg: WhisperCppConfig) -> Self {
        Self {
            cfg,
            model_path: None,
            loaded: false,
            inner: None,
        }
    }

    pub fn default_model_dir() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctl")
            .join("models")
    }

    fn resolve_model_path(&self) -> Result<PathBuf> {
        let size = &self.cfg.model_size;

        // Absolute path or ends with .bin → use directly
        if size.ends_with(".bin") || Path::new(size).is_absolute() {
            let p = PathBuf::from(size);
            if p.exists() {
                return Ok(p);
            }
            bail!("Model file not found: {}", p.display());
        }

        let model_dir = if self.cfg.model_dir.is_empty() {
            Self::default_model_dir()
        } else {
            PathBuf::from(&self.cfg.model_dir)
        };

        let candidates = GGUF_MAP
            .iter()
            .find(|(name, _)| *name == size.as_str())
            .map(|(_, files)| *files)
            .ok_or_else(|| anyhow::anyhow!("Unknown model size '{size}'"))?;

        for filename in candidates {
            let path = model_dir.join(filename);
            if path.exists() {
                return Ok(path);
            }
        }

        let first = candidates[0];
        bail!(
            "Model '{size}' not found in {}.\n\
             Download with:\n  wget -P {} {GGUF_BASE_URL}{first}",
            model_dir.display(),
            model_dir.display()
        )
    }

    fn threads(&self) -> u32 {
        if self.cfg.threads == 0 {
            (num_cpus() / 2).max(1)
        } else {
            self.cfg.threads
        }
    }
}

impl TranscriptionBackend for WhisperCppBackend {
    fn name(&self) -> &str {
        "whisper-cpp"
    }

    fn load(&mut self) -> Result<()> {
        let path = self.resolve_model_path()?;
        info!("Loading whisper.cpp model: {}", path.display());

        let mut params = whisper_rs::WhisperContextParameters::default();
        params.use_gpu = self.cfg.device.to_lowercase() != "cpu";

        let ctx = whisper_rs::WhisperContext::new_with_params(
            path.to_str().unwrap(),
            params,
        )
        .context("whisper-rs load")?;
        self.inner = Some(ctx);

        self.model_path = Some(path);
        self.loaded = true;
        Ok(())
    }

    fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
        if !self.loaded {
            bail!("Model not loaded");
        }

        if let Some(ctx) = &self.inner {
            return transcribe_bundled(ctx, req, self.threads());
        }
        bail!("Model context not initialized")
    }

    fn unload(&mut self) {
        self.inner = None;
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }
}

// ── whisper-rs bundled path ───────────────────────────────────────────────────

fn transcribe_bundled(
    ctx: &whisper_rs::WhisperContext,
    req: &TranscribeRequest,
    threads: u32,
) -> Result<TranscriptionResult> {
    use whisper_rs::{FullParams, SamplingStrategy};

    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(threads as i32);
    params.set_translate(false);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_timestamps(false);

    if let Some(lang) = &req.language {
        params.set_language(Some(lang));
    }
    if let Some(prompt) = &req.initial_prompt {
        params.set_initial_prompt(prompt);
    }

    let t0 = Instant::now();
    let mut state = ctx.create_state().context("whisper state")?;
    state.full(params, &req.audio).context("whisper full")?;
    let inference_ms = t0.elapsed().as_millis() as u32;

    let n = state.full_n_segments();
    let mut parts = Vec::new();
    for i in 0..n {
        if let Some(segment) = state.get_segment(i) {
            if let Ok(text) = segment.to_str() {
                parts.push(text.trim().to_string());
            }
        }
    }
    let text = parts.join(" ");
    let language = req.language.clone().unwrap_or_else(|| "en".into());

    Ok(TranscriptionResult {
        text,
        language,
        language_probability: 1.0,
        duration_ms: (req.audio.len() as u32) / 16,
        inference_ms,
        word_timestamps: None,
    })
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}

pub fn is_model_downloaded(size: &str) -> bool {
    let candidates = match GGUF_MAP.iter().find(|(name, _)| *name == size) {
        Some((_, files)) => *files,
        None => return false,
    };
    let model_dir = WhisperCppBackend::default_model_dir();
    candidates.iter().any(|filename| model_dir.join(filename).exists())
}

pub async fn download_model(size: &str) -> Result<()> {
    let candidates = GGUF_MAP
        .iter()
        .find(|(name, _)| *name == size)
        .map(|(_, files)| *files)
        .ok_or_else(|| anyhow::anyhow!("Unknown model size '{size}'"))?;

    let model_dir = WhisperCppBackend::default_model_dir();
    tokio::fs::create_dir_all(&model_dir).await?;

    let filename = candidates[0];
    let path = model_dir.join(filename);

    if path.exists() {
        return Ok(());
    }

    let url = format!("{}{}", GGUF_BASE_URL, filename);
    info!("Downloading Whisper model: {}", url);

    let response = reqwest::get(&url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    tokio::fs::write(&path, bytes)
        .await
        .context("save model file")?;

    info!("Whisper model downloaded successfully to: {}", path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxctr_config::WhisperCppConfig;

    #[test]
    fn test_new_backend() {
        let cfg = WhisperCppConfig {
            model_dir: "/tmp".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 4,
        };
        let backend = WhisperCppBackend::new(cfg);
        assert_eq!(backend.name(), "whisper-cpp");
        assert!(!backend.is_loaded());
    }

    #[test]
    fn test_threads_calculation() {
        // Explicit threads count
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 5,
        };
        let backend = WhisperCppBackend::new(cfg);
        assert_eq!(backend.threads(), 5);

        // Auto threads count (0)
        let cfg_auto = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend_auto = WhisperCppBackend::new(cfg_auto);
        assert!(backend_auto.threads() >= 1);
    }

    #[test]
    fn test_resolve_model_path_absolute() {
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "/tmp/nonexistent.bin".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        // Should bail because path does not exist
        assert!(backend.resolve_model_path().is_err());
    }

    #[test]
    fn test_resolve_model_path_unknown_size() {
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "invalid_size".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        assert!(backend.resolve_model_path().is_err());
    }

    #[test]
    fn test_transcribe_unloaded_error() {
        let cfg = WhisperCppConfig {
            model_dir: "".to_string(),
            model_size: "tiny".to_string(),
            device: "cpu".to_string(),
            threads: 0,
        };
        let backend = WhisperCppBackend::new(cfg);
        let req = crate::backend::TranscribeRequest {
            audio: vec![0.0; 16000],
            language: None,
            initial_prompt: None,
        };
        assert!(backend.transcribe(&req).is_err());
    }
}

