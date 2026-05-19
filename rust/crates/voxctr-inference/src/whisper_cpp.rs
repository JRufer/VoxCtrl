use std::{
    collections::HashMap,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    time::Instant,
};

use anyhow::{bail, Context, Result};
use tracing::{debug, info};
use voxctr_config::WhisperCppConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult, WordTimestamp};

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

    /// whisper-rs in-process model (only when compiled with feature whisper-bundled)
    #[cfg(feature = "whisper-bundled")]
    inner: Option<whisper_rs::WhisperContext>,
}

impl WhisperCppBackend {
    pub fn new(cfg: WhisperCppConfig) -> Self {
        Self {
            cfg,
            model_path: None,
            loaded: false,
            #[cfg(feature = "whisper-bundled")]
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

        #[cfg(feature = "whisper-bundled")]
        {
            use whisper_rs::{WhisperContext, WhisperContextParameters};
            let ctx = WhisperContext::new_with_params(
                path.to_str().unwrap(),
                WhisperContextParameters::default(),
            )
            .context("whisper-rs load")?;
            self.inner = Some(ctx);
        }

        self.model_path = Some(path);
        self.loaded = true;
        Ok(())
    }

    fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
        if !self.loaded {
            bail!("Model not loaded");
        }

        #[cfg(feature = "whisper-bundled")]
        if let Some(ctx) = &self.inner {
            return transcribe_bundled(ctx, req, self.threads());
        }

        transcribe_subprocess(self, req)
    }

    fn unload(&mut self) {
        #[cfg(feature = "whisper-bundled")]
        {
            self.inner = None;
        }
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }
}

// ── Subprocess path ───────────────────────────────────────────────────────────

fn transcribe_subprocess(
    backend: &WhisperCppBackend,
    req: &TranscribeRequest,
) -> Result<TranscriptionResult> {
    let model_path = backend.model_path.as_ref().unwrap();

    let wav = samples_to_wav(&req.audio);

    let mut cmd = std::process::Command::new(&backend.cfg.binary);
    cmd.arg("--model")
        .arg(model_path)
        .arg("--output-json")
        .arg("--threads")
        .arg(backend.threads().to_string())
        .arg("--no-timestamps");

    if let Some(lang) = &req.language {
        cmd.arg("--language").arg(lang);
    }
    if let Some(prompt) = &req.initial_prompt {
        cmd.arg("--prompt").arg(prompt);
    }

    // Device flags
    match backend.cfg.device.as_str() {
        "cuda" => { cmd.arg("--gpu").arg("cuda"); }
        "vulkan" => { cmd.arg("--gpu").arg("vulkan"); }
        _ => {}
    }

    cmd.arg("--file").arg("-");

    let t0 = Instant::now();
    let output = cmd
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            child.stdin.as_mut().unwrap().write_all(&wav)?;
            child.wait_with_output()
        })
        .context("whisper-cli subprocess")?;

    let inference_ms = t0.elapsed().as_millis() as u32;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("whisper-cli failed: {stderr}");
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if stdout.is_empty() {
        return Ok(TranscriptionResult {
            text: String::new(),
            language: req.language.clone().unwrap_or_else(|| "en".into()),
            language_probability: 1.0,
            duration_ms: (req.audio.len() as u32) / 16,
            inference_ms,
            word_timestamps: None,
        });
    }

    parse_cpp_json(&stdout, req, inference_ms)
}

fn parse_cpp_json(
    json_str: &str,
    req: &TranscribeRequest,
    inference_ms: u32,
) -> Result<TranscriptionResult> {
    let data: serde_json::Value = serde_json::from_str(json_str)
        .unwrap_or(serde_json::Value::Null);

    let transcription = data["transcription"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|seg| seg["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| json_str.trim().to_string());

    let language = data["result"]["language"]
        .as_str()
        .unwrap_or(req.language.as_deref().unwrap_or("en"))
        .to_string();

    Ok(TranscriptionResult {
        text: transcription,
        language,
        language_probability: 1.0,
        duration_ms: (req.audio.len() as u32) / 16,
        inference_ms,
        word_timestamps: None,
    })
}

// ── whisper-rs bundled path ───────────────────────────────────────────────────

#[cfg(feature = "whisper-bundled")]
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

    let n = state.full_n_segments().context("n_segments")?;
    let mut parts = Vec::new();
    for i in 0..n {
        if let Ok(text) = state.full_get_segment_text(i) {
            parts.push(text.trim().to_string());
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

// ── WAV helpers ───────────────────────────────────────────────────────────────

fn samples_to_wav(samples: &[f32]) -> Vec<u8> {
    let pcm: Vec<i16> = samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect();
    let data_len = (pcm.len() * 2) as u32;
    let mut buf = Vec::with_capacity(44 + data_len as usize);
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVEfmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());  // PCM
    buf.extend_from_slice(&1u16.to_le_bytes());  // mono
    buf.extend_from_slice(&16000u32.to_le_bytes());
    buf.extend_from_slice(&32000u32.to_le_bytes()); // byte rate
    buf.extend_from_slice(&2u16.to_le_bytes());  // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for s in &pcm {
        buf.extend_from_slice(&s.to_le_bytes());
    }
    buf
}

fn num_cpus() -> u32 {
    std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
}
