//! Parakeet speech-to-text backend (ONNX Runtime).
//!
//! NVIDIA Parakeet TDT 0.6B (<https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3>)
//! is a multilingual FastConformer-TDT automatic speech recognition model.
//! Converted to ONNX by Ilya Stupakov (`istupakov/parakeet-tdt-0.6b-v3-onnx`).
//!
//! Unlike autoregressive sequence-to-sequence models (Whisper/Moonshine),
//! Parakeet uses a Token-and-Duration Transducer (TDT). The TDT decoder jointly
//! predicts the next token and how many encoder audio frames to advance, allowing
//! it to skip silence and non-speech in leaps without hallucination loops.
//!
//! The pipeline uses three ONNX graphs:
//! 1. `nemo128.onnx`                — raw 16 kHz audio `[1, samples]` → 128-mel features `[1, 128, T]`
//! 2. `encoder-model.int8.onnx`     — mel features `[1, 128, T]` → hidden states `[1, 1024, T']`
//! 3. `decoder_joint-model.int8.onnx` — TDT greedy transducer loop over frames `T'`
//!
//! Two derivatives of the same model run through this pipeline unchanged, only
//! with different weights: Moondream's 1.58-bit Parakeet Redux and Fermion
//! Research's English-only Phonon-2. Their exports use the same tensor names,
//! shapes and logit layout as the NVIDIA one; [`download_spec`] saves their
//! files under the local names above.
//!
//! Audio longer than [`CHUNK_MAX_SAMPLES`] is split at the quietest point
//! between 25 and 35 seconds and each piece is transcribed on its own (see
//! [`chunk_ranges`]), as Phonon-2's own runtime does. The encoder's attention
//! cost grows with the square of the input length, so one long buffer is both
//! slower and far heavier on memory than the same audio in windows.

use std::{
    fs::File,
    io::{BufRead, BufReader},
    ops::Range,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use anyhow::{anyhow, bail, Context, Result};
use ort::{
    session::{
        builder::{GraphOptimizationLevel, SessionBuilder},
        Session, SessionInputValue,
    },
    value::Tensor,
};
use tracing::info;
use voxctrl_config::ParakeetConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

// ── Model constants ───────────────────────────────────────────────────────────

pub const SAMPLE_RATE: usize = 16_000;
pub const BLANK_TOKEN_ID: usize = 8192;
pub const NUM_DURATION_CLASSES: usize = 5;
pub const MAX_TOKENS_PER_STEP: usize = 10;
pub const ENCODER_DIM: usize = 1024;
pub const DECODER_STATE_DIM: usize = 640;
pub const DECODER_NUM_LAYERS: usize = 2;

pub const CONFIG_FILE: &str = "config.json";
pub const VOCAB_FILE: &str = "vocab.txt";
pub const PREPROCESSOR_FILE: &str = "nemo128.onnx";
pub const ENCODER_FILE: &str = "encoder-model.int8.onnx";
pub const DECODER_FILE: &str = "decoder_joint-model.int8.onnx";

pub const MODEL_FILES: [&str; 5] = [
    CONFIG_FILE,
    VOCAB_FILE,
    PREPROCESSOR_FILE,
    ENCODER_FILE,
    DECODER_FILE,
];

pub const HF_BASE_URL: &str =
    "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main";

/// Model size id for Moondream's 1.58-bit ternary Parakeet Redux.
pub const REDUX_SIZE: &str = "tdt-0.6b-v3-redux";

/// Community ONNX export of `moondream/parakeet-redux` (ternary weights stored
/// as ONNX Runtime `MatMulNBits` blocks, bit-exact with the source). Same tensor
/// names and logit layout as the NVIDIA export, so the pipeline is unchanged.
const REDUX_BASE_URL: &str =
    "https://huggingface.co/eschmidbauer/parakeet-redux-onnx/resolve/main";

/// `(remote file, local file)` pairs for the Redux export. Remote names differ
/// from ours, so they are saved under the standard local names.
const REDUX_FILES: [(&str, &str); 5] = [
    ("config.json", CONFIG_FILE),
    ("vocab.txt", VOCAB_FILE),
    ("preprocessor.onnx", PREPROCESSOR_FILE),
    ("encoder-model.onnx", ENCODER_FILE),
    ("decoder_joint-model.onnx", DECODER_FILE),
];

/// Model size id for Fermion Research's Phonon-2, an English-only retrain of
/// Parakeet TDT 0.6B v3 with five-value (~2.1-bit) encoder weights
/// (<https://huggingface.co/FermionResearch/Phonon-2>).
pub const PHONON2_SIZE: &str = "phonon-2";

/// Community ONNX export of Phonon-2. Fermion ships its weights only in its own
/// `.bps` container, which needs their Python/MLX runtime; this export expands
/// them into the onnx-asr `nemo-conformer-tdt` layout, so it shares this
/// backend's tensor names, shapes and logit layout (8,192 pieces plus blank,
/// five duration classes).
const PHONON2_BASE_URL: &str =
    "https://huggingface.co/tiyuvta/Phonon-2-ONNX/resolve/main";

/// `(remote file, local file)` pairs for the Phonon-2 export.
///
/// - The preprocessor is Phonon-2's own front end (pre-emphasis and per-band
///   normalisation), not the stock NeMo one, so it has to come from this repo.
/// - `exact4x2` is the encoder reproducing Fermion's fp32 runtime token for
///   token at about half the size: each five-value row is stored as two ternary
///   planes in `MatMulNBits`, the same contrib op the Redux export already
///   relies on, so the bundled ONNX Runtime runs it as is.
/// - The decoder is the export's single fp32 decoder/joint graph.
const PHONON2_FILES: [(&str, &str); 5] = [
    ("config.json", CONFIG_FILE),
    ("vocab.txt", VOCAB_FILE),
    ("preprocessor-model.onnx", PREPROCESSOR_FILE),
    ("encoder-model.exact4x2.onnx", ENCODER_FILE),
    ("decoder_joint-model.onnx", DECODER_FILE),
];

/// True for model sizes that only transcribe English. The language setting
/// does not apply to them, and their results report `"en"`.
pub fn is_english_only(size: &str) -> bool {
    size == PHONON2_SIZE
}

/// Base URL and `(remote, local)` file list for a model size.
fn download_spec(size: &str) -> (&'static str, Vec<(&'static str, &'static str)>) {
    match size {
        REDUX_SIZE => (REDUX_BASE_URL, REDUX_FILES.to_vec()),
        PHONON2_SIZE => (PHONON2_BASE_URL, PHONON2_FILES.to_vec()),
        _ => (HF_BASE_URL, MODEL_FILES.iter().map(|f| (*f, *f)).collect()),
    }
}

// ── Filesystem layout ─────────────────────────────────────────────────────────

/// Default parent directory for Parakeet models: `<models_base_dir>/parakeet/`.
pub fn default_model_dir() -> PathBuf {
    crate::util::models_base_dir().join("parakeet")
}

/// Directory holding one model's files: `<model_dir>/<size>/`.
fn model_size_dir(model_dir: &str, size: &str) -> PathBuf {
    let base = if model_dir.is_empty() {
        default_model_dir()
    } else {
        crate::util::expand_tilde(model_dir)
    };
    base.join(size)
}

pub fn valid_model_size(size: &str) -> bool {
    matches!(size, "tdt-0.6b-v3" | "tdt-0.6b-v3-int8" | REDUX_SIZE | PHONON2_SIZE)
}

/// True when all 5 model files are present on disk.
pub fn is_model_downloaded(size: &str, model_dir: &str) -> bool {
    if !valid_model_size(size) {
        return false;
    }
    let dir = model_size_dir(model_dir, size);
    MODEL_FILES.iter().all(|f| dir.join(f).exists())
}

// ── Download ──────────────────────────────────────────────────────────────────

static DOWNLOAD_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Fetch the 5 model files for `size` into `<model_dir>/<size>/`.
pub async fn download_model(size: &str, model_dir: &str) -> Result<()> {
    if !valid_model_size(size) {
        bail!(
            "Unknown Parakeet model size '{size}' \
             (expected 'tdt-0.6b-v3', '{REDUX_SIZE}' or '{PHONON2_SIZE}')"
        );
    }

    let dir = model_size_dir(model_dir, size);
    tokio::fs::create_dir_all(&dir).await?;

    let _guard = DOWNLOAD_LOCK.lock().await;

    let (base_url, files) = download_spec(size);
    for (remote, local) in files {
        let path = dir.join(local);
        if path.exists() {
            continue;
        }
        let url = format!("{base_url}/{remote}");
        info!("Downloading Parakeet file: {url}");
        download_file(&url, &path).await?;
    }

    info!("Parakeet '{size}' model ready in {}", dir.display());
    Ok(())
}

/// Stream `url` to `path` through a `.part` file, renamed into place only once
/// the whole body has arrived.
///
/// Streaming keeps memory flat: the encoders are 400–660 MB, and buffering one
/// with `Response::bytes()` held all of it in RAM before a byte reached disk.
/// When the server sends a length, a body that ends short is rejected rather
/// than renamed into place, where `is_model_downloaded` would later have taken
/// it for a finished file. A failed attempt removes its `.part` file.
async fn download_file(url: &str, path: &Path) -> Result<()> {
    let tmp = path.with_extension("part");
    let result = stream_to(url, &tmp).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result?;
    tokio::fs::rename(&tmp, path)
        .await
        .with_context(|| format!("finalize {}", path.display()))
}

async fn stream_to(url: &str, tmp: &Path) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("fetch {url}"))?;
    let expected = response.content_length();

    let mut file = tokio::fs::File::create(tmp)
        .await
        .with_context(|| format!("create {}", tmp.display()))?;
    let mut written: u64 = 0;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.with_context(|| format!("read {url}"))?;
        file.write_all(&chunk)
            .await
            .with_context(|| format!("write {}", tmp.display()))?;
        written += chunk.len() as u64;
    }
    file.flush()
        .await
        .with_context(|| format!("flush {}", tmp.display()))?;
    file.sync_all()
        .await
        .with_context(|| format!("sync {}", tmp.display()))?;

    if let Some(expected) = expected {
        if written != expected {
            bail!("download of {url} ended early: got {written} of {expected} bytes");
        }
    }
    Ok(())
}

// ── Vocabulary ────────────────────────────────────────────────────────────────

fn load_vocab(path: &Path) -> Result<Vec<String>> {
    let file = File::open(path).with_context(|| format!("open vocab file {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut entries: Vec<(usize, String)> = Vec::new();
    let mut max_id = 0;

    for (line_idx, line_res) in reader.lines().enumerate() {
        let line = line_res?;
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        // Format can be "<token> <id>" or simply "<token>" (index = line_idx)
        if let Some(pos) = trimmed.rfind(' ') {
            let (token, id_str) = trimmed.split_at(pos);
            if let Ok(id) = id_str.trim().parse::<usize>() {
                max_id = max_id.max(id);
                entries.push((id, token.to_string()));
                continue;
            }
        }
        max_id = max_id.max(line_idx);
        entries.push((line_idx, trimmed.to_string()));
    }

    let mut vocab = vec![String::new(); max_id + 1];
    for (id, token) in entries {
        if id < vocab.len() {
            vocab[id] = token;
        }
    }
    Ok(vocab)
}

fn detokenize(tokens: &[usize], vocab: &[String]) -> String {
    let mut raw = String::new();
    for &tok_id in tokens {
        if tok_id >= vocab.len() {
            continue;
        }
        let tok = &vocab[tok_id];
        if tok == "<blk>" || tok == "<unk>" || tok == "<pad>" || tok.starts_with("<|") || tok.is_empty() {
            continue;
        }
        raw.push_str(tok);
    }
    let converted = raw.replace('\u{2581}', " ").replace('Ġ', " ");
    converted.trim().to_string()
}

// ── Loaded State ──────────────────────────────────────────────────────────────

struct Loaded {
    preprocessor: Session,
    encoder: Session,
    decoder: Session,
    vocab: Vec<String>,
    targets_is_i32: bool,
    decoder_enc_shape_time_first: bool,
    logits_idx: usize,
    state_1_idx: usize,
    state_2_idx: usize,
}

// ── Backend ───────────────────────────────────────────────────────────────────

pub struct ParakeetBackend {
    cfg: ParakeetConfig,
    state: Mutex<Option<Loaded>>,
    loaded: bool,
}

impl ParakeetBackend {
    pub fn new(cfg: ParakeetConfig) -> Self {
        Self {
            cfg,
            state: Mutex::new(None),
            loaded: false,
        }
    }

    fn build_session(path: &Path, use_gpu: bool) -> Result<Session> {
        let builder = Session::builder()
            .map_err(|e| anyhow!("ort session builder: {e}"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| anyhow!("set optimization level: {e}"))?
            .with_intra_threads(crate::util::inference_threads())
            .map_err(|e| anyhow!("set intra threads: {e}"))?;

        let mut builder = Self::with_gpu(builder, use_gpu);

        builder
            .commit_from_file(path)
            .with_context(|| format!("load ONNX graph {}", path.display()))
    }

    fn with_gpu(builder: SessionBuilder, use_gpu: bool) -> SessionBuilder {
        // The user's Device setting: "cpu" skips provider registration entirely.
        if !use_gpu {
            return builder;
        }
        #[cfg(any(
            feature = "parakeet-cuda",
            feature = "parakeet-coreml",
            feature = "parakeet-webgpu"
        ))]
        {
            #[cfg(feature = "parakeet-cuda")]
            let ep = ort::ep::CUDA::default().build().error_on_failure();
            #[cfg(all(feature = "parakeet-coreml", not(feature = "parakeet-cuda")))]
            let ep = ort::ep::CoreML::default().build().error_on_failure();
            #[cfg(all(
                feature = "parakeet-webgpu",
                not(any(feature = "parakeet-cuda", feature = "parakeet-coreml"))
            ))]
            let ep = ort::ep::WebGPU::default().build().error_on_failure();

            let name = crate::parakeet_gpu_provider().unwrap_or("GPU");

            match builder.with_execution_providers([ep]) {
                Ok(with_ep) => {
                    tracing::debug!("Parakeet: {name} execution provider registered");
                    return with_ep;
                }
                Err(e) => {
                    tracing::warn!(
                        "Parakeet: {name} execution provider unavailable ({e}); running on CPU"
                    );
                    return e.recover();
                }
            }
        }

        #[cfg(not(any(
            feature = "parakeet-cuda",
            feature = "parakeet-coreml",
            feature = "parakeet-webgpu"
        )))]
        builder
    }
}

impl TranscriptionBackend for ParakeetBackend {
    fn name(&self) -> &str {
        "parakeet"
    }

    fn load(&mut self) -> Result<()> {
        let size = &self.cfg.model_size;
        let dir = model_size_dir("", size);
        if !is_model_downloaded(size, "") {
            bail!(
                "Parakeet '{size}' model is not downloaded (expected {} in {}). \
                 Open Settings → Engine and download it, or place the files there manually.",
                MODEL_FILES.join(", "),
                dir.display()
            );
        }

        info!("Loading Parakeet '{size}' model from {}", dir.display());
        let use_gpu = self.cfg.device != "cpu";
        match crate::parakeet_gpu_backend() {
            Some(_) if !use_gpu => info!("Parakeet acceleration: none (Device set to CPU)"),
            Some(backend) => info!("Parakeet acceleration: {backend}"),
            None => info!("Parakeet acceleration: none (CPU); this build has no ONNX Runtime GPU provider"),
        }

        let preprocessor = Self::build_session(&dir.join(PREPROCESSOR_FILE), use_gpu)?;
        let encoder = Self::build_session(&dir.join(ENCODER_FILE), use_gpu)?;
        let decoder = Self::build_session(&dir.join(DECODER_FILE), use_gpu)?;

        let vocab = load_vocab(&dir.join(VOCAB_FILE))?;

        let targets_is_i32 = decoder
            .inputs()
            .iter()
            .find(|i| i.name() == "targets")
            .map(|i| match i.dtype() {
                ort::value::ValueType::Tensor { ty, .. } => {
                    *ty == ort::value::TensorElementType::Int32
                }
                _ => true,
            })
            .unwrap_or(true);

        let decoder_enc_shape_time_first = decoder
            .inputs()
            .iter()
            .find(|i| i.name() == "encoder_outputs")
            .map(|i| match i.dtype() {
                ort::value::ValueType::Tensor { shape, .. } => {
                    shape.get(2).copied() == Some(ENCODER_DIM as i64)
                }
                _ => false,
            })
            .unwrap_or(false);

        let logits_idx = decoder
            .outputs()
            .iter()
            .position(|o| o.name() == "outputs")
            .unwrap_or(0);

        let state_1_idx = decoder
            .outputs()
            .iter()
            .position(|o| o.name() == "output_states_1")
            .unwrap_or(2);

        let state_2_idx = decoder
            .outputs()
            .iter()
            .position(|o| o.name() == "output_states_2")
            .unwrap_or(3);

        *self.state.lock().unwrap() = Some(Loaded {
            preprocessor,
            encoder,
            decoder,
            vocab,
            targets_is_i32,
            decoder_enc_shape_time_first,
            logits_idx,
            state_1_idx,
            state_2_idx,
        });
        self.loaded = true;
        Ok(())
    }

    fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
        if !self.loaded {
            bail!("Model not loaded");
        }
        let mut guard = self.state.lock().unwrap();
        let state = guard.as_mut().context("Parakeet state not initialised")?;

        let n_samples = req.audio.len();
        if n_samples == 0 {
            return Ok(TranscriptionResult {
                text: String::new(),
                language: self.cfg.language.clone(),
                language_probability: 1.0,
                duration_ms: 0,
                inference_ms: 0,
                word_timestamps: None,
            });
        }

        let t0 = Instant::now();
        let ranges = chunk_ranges(&req.audio);
        if ranges.len() > 1 {
            tracing::debug!(
                "Parakeet: {:.1} s of audio split into {} windows",
                n_samples as f32 / SAMPLE_RATE as f32,
                ranges.len()
            );
        }
        let mut parts: Vec<String> = Vec::with_capacity(ranges.len());
        for range in ranges {
            let part = run_inference(state, &req.audio[range])?;
            let part = part.trim();
            if !part.is_empty() {
                parts.push(part.to_string());
            }
        }
        let text = parts.join(" ");
        let inference_ms = t0.elapsed().as_millis() as u32;

        let language = if is_english_only(&self.cfg.model_size) {
            "en".to_string()
        } else {
            self.cfg.language.clone()
        };

        Ok(TranscriptionResult {
            text,
            language,
            language_probability: 1.0,
            duration_ms: (n_samples / (SAMPLE_RATE / 1000)) as u32,
            inference_ms,
            word_timestamps: None,
        })
    }

    fn unload(&mut self) {
        *self.state.lock().unwrap() = None;
        self.loaded = false;
    }

    fn is_loaded(&self) -> bool {
        self.loaded
    }
}

fn argmax(slice: &[f32]) -> usize {
    let mut best_idx = 0;
    let mut best_val = f32::NEG_INFINITY;
    for (i, &val) in slice.iter().enumerate() {
        if val > best_val {
            best_val = val;
            best_idx = i;
        }
    }
    best_idx
}

fn dims(shape: &[i64]) -> Vec<usize> {
    shape.iter().map(|&d| d as usize).collect()
}

// ── Long-audio chunking ───────────────────────────────────────────────────────

/// A window never ends before this many samples (25 s) unless the audio does.
pub const CHUNK_MIN_SAMPLES: usize = 25 * SAMPLE_RATE;
/// No window is longer than this (35 s); audio up to this length is one window.
pub const CHUNK_MAX_SAMPLES: usize = 35 * SAMPLE_RATE;
/// The last window is at least this long (2 s), so a cut never strands a
/// sliver of audio too short to hold a word.
const CHUNK_MIN_TAIL_SAMPLES: usize = 2 * SAMPLE_RATE;
/// Energy is measured over this span (200 ms) centred on each candidate cut:
/// long enough to find a pause between words rather than a quiet sample
/// inside one.
const CUT_WINDOW_SAMPLES: usize = SAMPLE_RATE / 5;
/// Distance between candidate cuts (10 ms).
const CUT_STEP_SAMPLES: usize = SAMPLE_RATE / 100;

/// Split `audio` into consecutive windows of 25–35 s, cutting each at the
/// quietest 200 ms in that range so that cuts land in pauses between words.
///
/// The windows are contiguous and cover every sample once. Audio no longer than
/// [`CHUNK_MAX_SAMPLES`] comes back as a single window.
pub(crate) fn chunk_ranges(audio: &[f32]) -> Vec<Range<usize>> {
    let n = audio.len();
    if n <= CHUNK_MAX_SAMPLES {
        return vec![0..n];
    }

    let half = CUT_WINDOW_SAMPLES / 2;
    let mut ranges = Vec::new();
    let mut start = 0;

    while n - start > CHUNK_MAX_SAMPLES {
        // n - start > 35 s, so hi - lo >= 35 - 2 - 25 > 0 seconds.
        let lo = start + CHUNK_MIN_SAMPLES;
        let hi = (start + CHUNK_MAX_SAMPLES).min(n - CHUNK_MIN_TAIL_SAMPLES);

        // Prefix sums of squared samples over just the span the candidates'
        // windows reach, so memory stays bounded by one window's worth.
        let span_lo = lo.saturating_sub(half);
        let span_hi = (hi + half).min(n);
        let mut prefix = Vec::with_capacity(span_hi - span_lo + 1);
        let mut acc = 0.0f64;
        prefix.push(acc);
        for &s in &audio[span_lo..span_hi] {
            acc += f64::from(s) * f64::from(s);
            prefix.push(acc);
        }
        let energy = |centre: usize| {
            let a = centre.saturating_sub(half).max(span_lo) - span_lo;
            let b = (centre + half).min(span_hi) - span_lo;
            prefix[b] - prefix[a]
        };

        // Strictly-less keeps the earliest of equally quiet points, so pure
        // silence is cut at 25 s and leaves the most room for what follows.
        let mut best = lo;
        let mut best_energy = f64::INFINITY;
        let mut cut = lo;
        while cut <= hi {
            let e = energy(cut);
            if e < best_energy {
                best_energy = e;
                best = cut;
            }
            cut += CUT_STEP_SAMPLES;
        }

        ranges.push(start..best);
        start = best;
    }

    ranges.push(start..n);
    ranges
}

fn run_inference(state: &mut Loaded, audio: &[f32]) -> Result<String> {
    let n_samples = audio.len();

    // ── 1. Preprocessor: Waveform → 128-channel Mel Spectrogram ──────────────
    let wave_tensor = Tensor::from_array(([1_usize, n_samples], audio.to_vec()))
        .context("build audio waveform tensor")?;
    let wave_lens_tensor = Tensor::from_array(([1_usize], vec![n_samples as i64]))
        .context("build audio waveform_lens tensor")?;

    let prep_feed: Vec<(&str, SessionInputValue)> = vec![
        ("waveforms", wave_tensor.into()),
        ("waveforms_lens", wave_lens_tensor.into()),
    ];

    let prep_out = state.preprocessor.run(prep_feed).context("preprocessor run")?;
    let (feat_shape, feat_data) = prep_out[0]
        .try_extract_tensor::<f32>()
        .context("extract mel features")?;
    let feat_shape_vec = feat_shape.to_vec();
    let feat_data_vec = feat_data.to_vec();

    let num_mel_frames = if feat_shape_vec.len() >= 3 {
        feat_shape_vec[2] as i64
    } else {
        (n_samples / 160) as i64
    };

    let feat_lens_vec = if prep_out.len() > 1 {
        let (_, lens_data) = prep_out[1]
            .try_extract_tensor::<i64>()
            .context("extract mel features_lens")?;
        lens_data.to_vec()
    } else {
        vec![num_mel_frames]
    };

    // ── 2. Encoder: Mel Spectrogram → Conformer Acoustic Features ────────────
    let audio_signal_tensor =
        Tensor::from_array((dims(&feat_shape_vec), feat_data_vec))
            .context("build audio_signal tensor")?;
    let length_tensor =
        Tensor::from_array(([1_usize], feat_lens_vec))
            .context("build length tensor")?;

    let enc_feed: Vec<(&str, SessionInputValue)> = vec![
        ("audio_signal", audio_signal_tensor.into()),
        ("length", length_tensor.into()),
    ];

    let enc_out = state.encoder.run(enc_feed).context("encoder run")?;
    let (enc_shape, enc_data) = enc_out[0]
        .try_extract_tensor::<f32>()
        .context("extract encoder outputs")?;

    // Determine time dimension T'
    // enc_shape can be [1, 1024, T'] (channels first) or [1, T', 1024] (time first)
    let channels_first = enc_shape.len() >= 3 && enc_shape[1] == ENCODER_DIM as i64;
    let t_prime = if channels_first {
        enc_shape[2] as usize
    } else if enc_shape.len() >= 3 {
        enc_shape[1] as usize
    } else {
        bail!("Unexpected encoder output rank: {:?}", enc_shape);
    };

    if t_prime == 0 {
        return Ok(String::new());
    }

    // ── 3. Decoder: TDT Greedy Search Loop ───────────────────────────────────
    let vocab_size = state.vocab.len();
    let output_dim = vocab_size + NUM_DURATION_CLASSES;
    let blank_idx = BLANK_TOKEN_ID;

    let mut state_1 = vec![0.0f32; DECODER_NUM_LAYERS * 1 * DECODER_STATE_DIM];
    let mut state_2 = vec![0.0f32; DECODER_NUM_LAYERS * 1 * DECODER_STATE_DIM];
    let mut current_token = blank_idx;

    let mut emitted_tokens: Vec<usize> = Vec::new();
    let mut t = 0;

    let enc_slice_shape = if state.decoder_enc_shape_time_first {
        vec![1_usize, 1, ENCODER_DIM]
    } else {
        vec![1_usize, ENCODER_DIM, 1]
    };

    let mut enc_frame = vec![0.0f32; ENCODER_DIM];

    while t < t_prime {
        // Extract encoder frame slice for time index t
        if channels_first {
            for c in 0..ENCODER_DIM {
                enc_frame[c] = enc_data[c * t_prime + t];
            }
        } else {
            let offset = t * ENCODER_DIM;
            enc_frame.copy_from_slice(&enc_data[offset..offset + ENCODER_DIM]);
        }

        let mut tokens_in_frame = 0;

        loop {
            let enc_tensor = Tensor::from_array((enc_slice_shape.clone(), enc_frame.clone()))
                .context("build decoder encoder_outputs tensor")?;
            let s1_tensor = Tensor::from_array((
                [DECODER_NUM_LAYERS, 1_usize, DECODER_STATE_DIM],
                state_1.clone(),
            ))
            .context("build decoder state_1 tensor")?;
            let s2_tensor = Tensor::from_array((
                [DECODER_NUM_LAYERS, 1_usize, DECODER_STATE_DIM],
                state_2.clone(),
            ))
            .context("build decoder state_2 tensor")?;

            let (target_input, target_len_input) = if state.targets_is_i32 {
                (
                    Tensor::from_array(([1_usize, 1], vec![current_token as i32]))
                        .context("build targets tensor")?
                        .into(),
                    Tensor::from_array(([1_usize], vec![1_i32]))
                        .context("build target_length tensor")?
                        .into(),
                )
            } else {
                (
                    Tensor::from_array(([1_usize, 1], vec![current_token as i64]))
                        .context("build targets tensor")?
                        .into(),
                    Tensor::from_array(([1_usize], vec![1_i64]))
                        .context("build target_length tensor")?
                        .into(),
                )
            };

            let dec_feed: Vec<(&str, SessionInputValue)> = vec![
                ("encoder_outputs", enc_tensor.into()),
                ("targets", target_input),
                ("target_length", target_len_input),
                ("input_states_1", s1_tensor.into()),
                ("input_states_2", s2_tensor.into()),
            ];

            let dec_out = state.decoder.run(dec_feed).context("decoder step run")?;
            let (_, ldata) = dec_out[state.logits_idx]
                .try_extract_tensor::<f32>()
                .context("extract decoder logits")?;

            let row_offset = if ldata.len() >= output_dim {
                ldata.len() - output_dim
            } else {
                0
            };
            let row = &ldata[row_offset..];
            let token_logits = &row[..vocab_size];
            let duration_logits = &row[vocab_size..vocab_size + NUM_DURATION_CLASSES];

            let best_token = argmax(token_logits);
            let best_duration = argmax(duration_logits);

            if best_token != blank_idx {
                emitted_tokens.push(best_token);
                current_token = best_token;

                let (_, next_s1) = dec_out[state.state_1_idx]
                    .try_extract_tensor::<f32>()
                    .context("extract next state_1")?;
                let (_, next_s2) = dec_out[state.state_2_idx]
                    .try_extract_tensor::<f32>()
                    .context("extract next state_2")?;
                state_1 = next_s1.to_vec();
                state_2 = next_s2.to_vec();

                tokens_in_frame += 1;
                if best_duration > 0 || tokens_in_frame >= MAX_TOKENS_PER_STEP {
                    t += best_duration.max(1);
                    break;
                }
            } else {
                t += best_duration.max(1);
                break;
            }
        }
    }

    Ok(detokenize(&emitted_tokens, &state.vocab))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_model_size() {
        assert!(valid_model_size("tdt-0.6b-v3"));
        assert!(valid_model_size("tdt-0.6b-v3-int8"));
        assert!(valid_model_size(REDUX_SIZE));
        assert!(valid_model_size(PHONON2_SIZE));
        assert!(!valid_model_size("base"));
        assert!(!valid_model_size(""));
    }

    #[test]
    fn test_english_only() {
        assert!(is_english_only(PHONON2_SIZE));
        assert!(!is_english_only("tdt-0.6b-v3"));
        assert!(!is_english_only(REDUX_SIZE));
    }

    /// Every size must save exactly the five local files `load()` and
    /// `is_model_downloaded()` look for, each from one remote file.
    #[test]
    fn test_download_spec_covers_local_files() {
        for size in ["tdt-0.6b-v3", "tdt-0.6b-v3-int8", REDUX_SIZE, PHONON2_SIZE] {
            let (base, files) = download_spec(size);
            assert!(base.starts_with("https://huggingface.co/"), "{size}: {base}");
            let mut locals: Vec<&str> = files.iter().map(|(_, l)| *l).collect();
            locals.sort_unstable();
            let mut expected = MODEL_FILES.to_vec();
            expected.sort_unstable();
            assert_eq!(locals, expected, "{size}");
        }
    }

    #[test]
    fn test_phonon2_download_spec() {
        let (base, files) = download_spec(PHONON2_SIZE);
        assert_eq!(base, PHONON2_BASE_URL);
        let remote_for = |local: &str| {
            files.iter().find(|(_, l)| *l == local).map(|(r, _)| *r).unwrap()
        };
        assert_eq!(remote_for(PREPROCESSOR_FILE), "preprocessor-model.onnx");
        assert_eq!(remote_for(ENCODER_FILE), "encoder-model.exact4x2.onnx");
        assert_eq!(remote_for(DECODER_FILE), "decoder_joint-model.onnx");
    }

    /// Deterministic "speech": a loud tone everywhere except the given
    /// silent gaps, each `(start_secs, len_secs)`.
    fn tone_with_gaps(total_secs: f32, gaps: &[(f32, f32)]) -> Vec<f32> {
        let n = (total_secs * SAMPLE_RATE as f32) as usize;
        (0..n)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let silent = gaps.iter().any(|&(s, l)| t >= s && t < s + l);
                if silent {
                    0.0
                } else {
                    0.5 * (2.0 * std::f32::consts::PI * 220.0 * t).sin()
                }
            })
            .collect()
    }

    fn assert_contiguous(ranges: &[Range<usize>], n: usize) {
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, n);
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "gap or overlap in {ranges:?}");
        }
        for r in ranges {
            assert!(r.len() <= CHUNK_MAX_SAMPLES, "window too long: {r:?}");
        }
    }

    #[test]
    fn test_chunk_ranges_short_audio_is_one_window() {
        assert_eq!(chunk_ranges(&[]), vec![0..0]);
        let audio = vec![0.1f32; CHUNK_MAX_SAMPLES];
        assert_eq!(chunk_ranges(&audio), vec![0..CHUNK_MAX_SAMPLES]);
    }

    #[test]
    fn test_chunk_ranges_cut_in_pauses() {
        // Pauses at 30 s and 58 s: both sit inside the 25–35 s search range of
        // their window, so each cut should land in one.
        let audio = tone_with_gaps(80.0, &[(30.0, 0.5), (58.0, 0.5)]);
        let ranges = chunk_ranges(&audio);
        assert_contiguous(&ranges, audio.len());
        assert_eq!(ranges.len(), 3, "{ranges:?}");
        let secs = |s: usize| s as f32 / SAMPLE_RATE as f32;
        let cut1 = secs(ranges[0].end);
        let cut2 = secs(ranges[1].end);
        assert!((30.0..30.5).contains(&cut1), "first cut at {cut1}s");
        assert!((58.0..58.5).contains(&cut2), "second cut at {cut2}s");
    }

    #[test]
    fn test_chunk_ranges_without_pauses() {
        // No quiet point at all: windows still respect the bounds.
        let audio = tone_with_gaps(100.0, &[]);
        let ranges = chunk_ranges(&audio);
        assert_contiguous(&ranges, audio.len());
        for r in &ranges[..ranges.len() - 1] {
            assert!(r.len() >= CHUNK_MIN_SAMPLES, "window too short: {r:?}");
        }
    }

    #[test]
    fn test_chunk_ranges_never_strands_a_short_tail() {
        // 35.5 s of silence: the only way to split it leaves a tail, which
        // must be at least the minimum length.
        let audio = vec![0.0f32; CHUNK_MAX_SAMPLES + SAMPLE_RATE / 2];
        let ranges = chunk_ranges(&audio);
        assert_contiguous(&ranges, audio.len());
        assert!(ranges.last().unwrap().len() >= CHUNK_MIN_TAIL_SAMPLES, "{ranges:?}");
    }

    #[test]
    fn test_detokenize_sentencepiece() {
        let vocab = vec![
            "Hello".to_string(),
            " world".to_string(),
            "!".to_string(),
        ];
        let tokens = vec![0, 1, 2];
        let text = detokenize(&tokens, &vocab);
        assert_eq!(text, "Hello world!");
    }
    #[test]
    fn test_inspect_decoder() {
        let path = std::path::Path::new("/home/jrufer/.local/share/voxctrl/models/parakeet/tdt-0.6b-v3/decoder_joint-model.int8.onnx");
        if !path.exists() {
            return;
        }
        let session = ParakeetBackend::build_session(path, false).unwrap();
        let outputs = session.outputs();
        let s1_idx = outputs.iter().position(|o| o.name() == "output_states_1").unwrap_or(2);
        let s2_idx = outputs.iter().position(|o| o.name() == "output_states_2").unwrap_or(3);
        let logits_idx = outputs.iter().position(|o| o.name() == "outputs").unwrap_or(0);
        assert_eq!(logits_idx, 0);
        assert_eq!(s1_idx, 2);
        assert_eq!(s2_idx, 3);
    }

    #[test]
    fn test_parakeet_transcribe_silence() {
        let _dir = model_size_dir("", "tdt-0.6b-v3");
        if !is_model_downloaded("tdt-0.6b-v3", "") {
            return;
        }
        let mut backend = ParakeetBackend::new(ParakeetConfig::default());
        backend.load().expect("load parakeet");
        let req = TranscribeRequest {
            audio: vec![0.0f32; 16000],
            language: None,
            word_timestamps: false,
            initial_prompt: None,
        };
        let res = backend.transcribe(&req).expect("transcribe silence");
        println!("Parakeet transcribe silence result: {:?}", res.text);
        assert_eq!(res.text, "");
    }

    /// Network + large download; run manually with `--ignored`.
    #[test]
    #[ignore]
    fn test_redux_download_and_transcribe() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(download_model(REDUX_SIZE, "")).expect("download redux");
        assert!(is_model_downloaded(REDUX_SIZE, ""));
        let mut cfg = ParakeetConfig::default();
        cfg.model_size = REDUX_SIZE.into();
        let mut backend = ParakeetBackend::new(cfg);
        backend.load().expect("load redux");
        let path = std::env::var("REDUX_TEST_PCM").expect("REDUX_TEST_PCM");
        let bytes = std::fs::read(path).unwrap();
        let audio: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let req = TranscribeRequest {
            audio,
            language: None,
            word_timestamps: false,
            initial_prompt: None,
        };
        let res = backend.transcribe(&req).expect("transcribe");
        println!("REDUX RESULT: {:?}", res.text);
        assert!(res.text.to_lowercase().contains("fox"));
    }

    /// Network + ~735 MB download; run manually with `--ignored`.
    ///
    /// `PHONON2_TEST_PCM` is raw little-endian f32 mono at 16 kHz, e.g.
    /// `ffmpeg -i clip.wav -f f32le -ac 1 -ar 16000 clip.pcm`. Use a clip
    /// longer than 35 s to exercise the chunked path as well.
    #[test]
    #[ignore]
    fn test_phonon2_download_and_transcribe() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(download_model(PHONON2_SIZE, "")).expect("download phonon-2");
        assert!(is_model_downloaded(PHONON2_SIZE, ""));
        let mut cfg = ParakeetConfig::default();
        cfg.model_size = PHONON2_SIZE.into();
        let mut backend = ParakeetBackend::new(cfg);
        backend.load().expect("load phonon-2");
        let path = std::env::var("PHONON2_TEST_PCM").expect("PHONON2_TEST_PCM");
        let bytes = std::fs::read(path).unwrap();
        let audio: Vec<f32> = bytes
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let req = TranscribeRequest {
            audio,
            language: None,
            word_timestamps: false,
            initial_prompt: None,
        };
        let res = backend.transcribe(&req).expect("transcribe");
        println!("PHONON-2 RESULT: {:?}", res.text);
        assert_eq!(res.language, "en");
        assert!(!res.text.is_empty());
    }
}
