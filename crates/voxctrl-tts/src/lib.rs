use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use tracing::{debug, info, warn};
use voxctrl_config::{TtsConfig, TtsEngine};

// ── Piper voice catalogue ─────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub quality: &'static str,
    pub sample_rate: u32,
    pub filename: &'static str,
}

pub static PIPER_VOICES: &[VoiceInfo] = &[
    VoiceInfo { name: "en-us-libritts-high",   quality: "high",   sample_rate: 22050, filename: "en_US-libritts-high.onnx" },
    VoiceInfo { name: "en-us-amy-low",         quality: "low",    sample_rate: 16000, filename: "en_US-amy-low.onnx" },
    VoiceInfo { name: "en-us-kathleen-low",    quality: "low",    sample_rate: 16000, filename: "en_US-kathleen-low.onnx" },
    VoiceInfo { name: "en-gb-southern_english_female-low", quality: "low", sample_rate: 16000, filename: "en_GB-southern_english_female-low.onnx" },
    VoiceInfo { name: "en-us-ryan-high",       quality: "high",   sample_rate: 22050, filename: "en_US-ryan-high.onnx" },
    VoiceInfo { name: "en-us-ryan-medium",     quality: "medium", sample_rate: 22050, filename: "en_US-ryan-medium.onnx" },
    VoiceInfo { name: "en-us-ryan-low",        quality: "low",    sample_rate: 16000, filename: "en_US-ryan-low.onnx" },
    VoiceInfo { name: "en-us-lessac-medium",   quality: "medium", sample_rate: 16000, filename: "en_US-lessac-medium.onnx" },
    VoiceInfo { name: "en-us-lessac-low",      quality: "low",    sample_rate: 16000, filename: "en_US-lessac-low.onnx" },
    VoiceInfo { name: "en-us-danny-low",       quality: "low",    sample_rate: 16000, filename: "en_US-danny-low.onnx" },
    VoiceInfo { name: "en-gb-alan-low",        quality: "low",    sample_rate: 16000, filename: "en_GB-alan-low.onnx" },
];

// ── Kokoro voice catalogue ────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct KokoroVoiceInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub lang: &'static str,
}

pub static KOKORO_VOICES: &[KokoroVoiceInfo] = &[
    // American Female
    KokoroVoiceInfo { id: "af_heart",    label: "Heart (American Female)",    lang: "en-us" },
    KokoroVoiceInfo { id: "af_bella",    label: "Bella (American Female)",    lang: "en-us" },
    KokoroVoiceInfo { id: "af_sarah",    label: "Sarah (American Female)",    lang: "en-us" },
    KokoroVoiceInfo { id: "af_nicole",   label: "Nicole (American Female)",   lang: "en-us" },
    KokoroVoiceInfo { id: "af_sky",      label: "Sky (American Female)",      lang: "en-us" },
    KokoroVoiceInfo { id: "af_alloy",    label: "Alloy (American Female)",    lang: "en-us" },
    KokoroVoiceInfo { id: "af_aoede",    label: "Aoede (American Female)",    lang: "en-us" },
    KokoroVoiceInfo { id: "af_jessica",  label: "Jessica (American Female)",  lang: "en-us" },
    KokoroVoiceInfo { id: "af_kore",     label: "Kore (American Female)",     lang: "en-us" },
    KokoroVoiceInfo { id: "af_nova",     label: "Nova (American Female)",     lang: "en-us" },
    KokoroVoiceInfo { id: "af_river",    label: "River (American Female)",    lang: "en-us" },
    // American Male
    KokoroVoiceInfo { id: "am_adam",     label: "Adam (American Male)",       lang: "en-us" },
    KokoroVoiceInfo { id: "am_michael",  label: "Michael (American Male)",    lang: "en-us" },
    KokoroVoiceInfo { id: "am_puck",     label: "Puck (American Male)",       lang: "en-us" },
    KokoroVoiceInfo { id: "am_echo",     label: "Echo (American Male)",       lang: "en-us" },
    KokoroVoiceInfo { id: "am_eric",     label: "Eric (American Male)",       lang: "en-us" },
    KokoroVoiceInfo { id: "am_fenrir",   label: "Fenrir (American Male)",     lang: "en-us" },
    KokoroVoiceInfo { id: "am_liam",     label: "Liam (American Male)",       lang: "en-us" },
    KokoroVoiceInfo { id: "am_onyx",     label: "Onyx (American Male)",       lang: "en-us" },
    KokoroVoiceInfo { id: "am_santa",    label: "Santa (American Male)",      lang: "en-us" },
    // British Female
    KokoroVoiceInfo { id: "bf_emma",     label: "Emma (British Female)",      lang: "en-gb" },
    KokoroVoiceInfo { id: "bf_alice",    label: "Alice (British Female)",     lang: "en-gb" },
    KokoroVoiceInfo { id: "bf_isabella", label: "Isabella (British Female)",  lang: "en-gb" },
    KokoroVoiceInfo { id: "bf_lily",     label: "Lily (British Female)",      lang: "en-gb" },
    // British Male
    KokoroVoiceInfo { id: "bm_george",   label: "George (British Male)",      lang: "en-gb" },
    KokoroVoiceInfo { id: "bm_lewis",    label: "Lewis (British Male)",       lang: "en-gb" },
    KokoroVoiceInfo { id: "bm_daniel",   label: "Daniel (British Male)",      lang: "en-gb" },
    KokoroVoiceInfo { id: "bm_fable",    label: "Fable (British Male)",       lang: "en-gb" },
];

// ── Kokoro vocabulary (IPA → token ID) ───────────────────────────────────────

// 114 entries extracted from kokoro-onnx config.json.
const KOKORO_VOCAB_PAIRS: &[(char, u32)] = &[
    (';', 1), (':', 2), (',', 3), ('.', 4), ('!', 5), ('?', 6),
    ('\u{2014}', 9),   // em-dash —
    ('\u{2026}', 10),  // horizontal ellipsis …
    ('"', 11),         // U+0022 plain quotation mark
    ('(', 12), (')', 13),
    ('\u{201C}', 14),  // U+201C left double quotation "
    ('\u{201D}', 15),  // U+201D right double quotation "
    (' ', 16),
    ('\u{0303}', 17),  // combining tilde ̃
    ('\u{02A3}', 18),  // ʣ
    ('\u{02A5}', 19),  // ʥ
    ('\u{02A6}', 20),  // ʦ
    ('\u{02A8}', 21),  // ʨ
    ('\u{1D5D}', 22),  // ᵝ
    ('\u{AB67}', 23),  // ꭧ
    ('A', 24), ('I', 25),
    ('O', 31), ('Q', 33), ('S', 35), ('T', 36), ('W', 39), ('Y', 41),
    ('\u{1D4A}', 42),  // ᵊ
    ('a', 43), ('b', 44), ('c', 45), ('d', 46), ('e', 47), ('f', 48),
    ('h', 50), ('i', 51), ('j', 52), ('k', 53), ('l', 54), ('m', 55),
    ('n', 56), ('o', 57), ('p', 58), ('q', 59), ('r', 60), ('s', 61),
    ('t', 62), ('u', 63), ('v', 64), ('w', 65), ('x', 66), ('y', 67),
    ('z', 68),
    ('\u{0251}', 69),  // ɑ
    ('\u{0250}', 70),  // ɐ
    ('\u{0252}', 71),  // ɒ
    ('\u{00E6}', 72),  // æ
    ('\u{03B2}', 75),  // β
    ('\u{0254}', 76),  // ɔ
    ('\u{0255}', 77),  // ɕ
    ('\u{00E7}', 78),  // ç
    ('\u{0256}', 80),  // ɖ
    ('\u{00F0}', 81),  // ð
    ('\u{02A4}', 82),  // ʤ
    ('\u{0259}', 83),  // ə
    ('\u{025A}', 85),  // ɚ
    ('\u{025B}', 86),  // ɛ
    ('\u{025C}', 87),  // ɜ
    ('\u{025F}', 90),  // ɟ
    ('\u{0261}', 92),  // ɡ
    ('\u{0265}', 99),  // ɥ
    ('\u{0268}', 101), // ɨ
    ('\u{026A}', 102), // ɪ
    ('\u{029D}', 103), // ʝ
    ('\u{026F}', 110), // ɯ
    ('\u{0270}', 111), // ɰ
    ('\u{014B}', 112), // ŋ
    ('\u{0273}', 113), // ɳ
    ('\u{0272}', 114), // ɲ
    ('\u{0274}', 115), // ɴ
    ('\u{00F8}', 116), // ø
    ('\u{0278}', 118), // ɸ
    ('\u{03B8}', 119), // θ
    ('\u{0153}', 120), // œ
    ('\u{0279}', 123), // ɹ
    ('\u{027E}', 125), // ɾ
    ('\u{027B}', 126), // ɻ
    ('\u{0281}', 128), // ʁ
    ('\u{027D}', 129), // ɽ
    ('\u{0282}', 130), // ʂ
    ('\u{0283}', 131), // ʃ
    ('\u{0288}', 132), // ʈ
    ('\u{02A7}', 133), // ʧ
    ('\u{028A}', 135), // ʊ
    ('\u{028B}', 136), // ʋ
    ('\u{028C}', 138), // ʌ
    ('\u{0263}', 139), // ɣ
    ('\u{0264}', 140), // ɤ
    ('\u{03C7}', 142), // χ
    ('\u{028E}', 143), // ʎ
    ('\u{0292}', 147), // ʒ
    ('\u{0294}', 148), // ʔ
    ('\u{02C8}', 156), // ˈ primary stress
    ('\u{02CC}', 157), // ˌ secondary stress
    ('\u{02D0}', 158), // ː long vowel
    ('\u{02B0}', 162), // ʰ aspirated
    ('\u{02B2}', 164), // ʲ palatalised
    ('\u{2193}', 169), // ↓
    ('\u{2192}', 171), // →
    ('\u{2197}', 172), // ↗
    ('\u{2198}', 173), // ↘
    ('\u{1D7B}', 177), // ᵻ
];

static KOKORO_VOCAB: OnceLock<HashMap<char, u32>> = OnceLock::new();

fn kokoro_vocab() -> &'static HashMap<char, u32> {
    KOKORO_VOCAB.get_or_init(|| KOKORO_VOCAB_PAIRS.iter().map(|&(c, id)| (c, id)).collect())
}

// ── Kokoro constants ──────────────────────────────────────────────────────────

const KOKORO_SAMPLE_RATE: u32 = 24000;

// Maximum inner-token count (excluding boundary 0s) before truncation.
const MAX_PHONEME_LENGTH: usize = 510;

// ── Kokoro tokenization ───────────────────────────────────────────────────────

/// Convert IPA phoneme string to Kokoro token IDs.
///
/// Returns `(tokens_for_model, num_inner_tokens)`.
/// `tokens_for_model` is `[0, ...phoneme ids..., 0]` (boundary pad tokens included).
/// `num_inner_tokens` is the count without the boundary 0s; used to index the style row.
pub fn kokoro_tokenize(phonemes: &str) -> (Vec<i64>, usize) {
    let vocab = kokoro_vocab();
    let cap = phonemes.len().min(MAX_PHONEME_LENGTH) + 2;
    let mut tokens = Vec::with_capacity(cap);
    tokens.push(0i64);
    tokens.extend(
        phonemes
            .chars()
            .filter_map(|ch| vocab.get(&ch).map(|&id| id as i64))
            .take(MAX_PHONEME_LENGTH),
    );
    let num_inner = tokens.len() - 1;
    tokens.push(0i64);
    (tokens, num_inner)
}

// ── Kokoro phonemization ──────────────────────────────────────────────────────

/// Convert text to IPA phonemes using espeak-ng.
///
/// `lang` is an espeak-ng voice identifier such as `"en-us"` or `"en-gb"`.
pub fn phonemize_espeak(text: &str, lang: &str) -> Result<String> {
    let output = std::process::Command::new("espeak-ng")
        .args(["--ipa", "-q", "-v", lang])
        .arg(text)
        .output()
        .context("espeak-ng not found; install it with: apt install espeak-ng")?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("espeak-ng failed: {}", err.trim());
    }

    let raw = String::from_utf8_lossy(&output.stdout);
    let phonemes = raw
        .lines()
        .map(|l| l.trim_start_matches('_').trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(phonemes)
}

// ── Kokoro voice embedding (NPZ / NPY) ───────────────────────────────────────

static EMBEDDING_CACHE: OnceLock<std::sync::Mutex<HashMap<String, Vec<f32>>>> = OnceLock::new();

/// Load one row from a voice's NPY array. It reads the standalone `.npy` file from the unzipped directory
/// and caches the full array in-memory for sub-microsecond retrieval on subsequent calls.
///
/// Each row is 256 float32 values forming the style embedding for that sequence length.
pub fn load_voice_embedding(voices_dir: &Path, voice: &str, row: usize) -> Result<Vec<f32>> {
    let cache_lock = EMBEDDING_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()));
    let mut cache = cache_lock.lock().unwrap();

    let full_data = if let Some(cached) = cache.get(voice) {
        cached
    } else {
        let voice_file_path = voices_dir.join(format!("{voice}.npy"));
        let mut file = std::fs::File::open(&voice_file_path)
            .with_context(|| format!("open voice file: {}", voice_file_path.display()))?;

        let mut data = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut data)?;

        if data.len() < 10 || &data[0..6] != b"\x93NUMPY" {
            anyhow::bail!("invalid NPY format for voice '{voice}'");
        }

        let major = data[6];
        let (header_len, header_offset): (usize, usize) = if major == 1 {
            (u16::from_le_bytes([data[8], data[9]]) as usize, 10)
        } else {
            (u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize, 12)
        };
        let data_start = header_offset + header_len;
        let payload = &data[data_start..];

        let floats: Vec<f32> = payload
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();

        cache.insert(voice.to_string(), floats);
        cache.get(voice).unwrap()
    };

    const COLS: usize = 256;
    let num_rows = full_data.len() / COLS;
    if num_rows == 0 {
        anyhow::bail!("NPY data empty for voice '{voice}'");
    }
    let clamped_row = row.min(num_rows - 1);
    let offset = clamped_row * COLS;

    if full_data.len() < offset + COLS {
        anyhow::bail!("NPY data too short for voice '{voice}' row {clamped_row}");
    }

    Ok(full_data[offset..offset + COLS].to_vec())
}

// ── ONNX Runtime initialisation ───────────────────────────────────────────────

/// Initialise the ONNX Runtime shared library (idempotent).
///
/// With the `load-dynamic` ort feature the library is loaded via dlopen at runtime.
/// We search standard locations, then fall back to the Python onnxruntime package.
fn ensure_ort_init() -> Result<()> {
    static ORT_INIT_DONE: OnceLock<Result<(), String>> = OnceLock::new();
    let result = ORT_INIT_DONE.get_or_init(|| {
        let try_init = || -> Result<()> {
            // If ORT_DYLIB_PATH is already set, use init_from().
            if let Ok(path) = std::env::var("ORT_DYLIB_PATH") {
                ort::init_from(&path)?;
                ort::init().commit();
                return Ok(());
            }

            // Search common locations for libonnxruntime.so.
            let candidates: &[&str] = &[
                "/usr/lib/libonnxruntime.so",
                "/usr/local/lib/libonnxruntime.so",
                "/usr/lib/x86_64-linux-gnu/libonnxruntime.so",
                "/usr/local/lib/python3.11/dist-packages/onnxruntime/capi/libonnxruntime.so.1.26.0",
                "/usr/local/lib/python3.12/dist-packages/onnxruntime/capi/libonnxruntime.so.1.26.0",
                "/usr/local/lib/python3.10/dist-packages/onnxruntime/capi/libonnxruntime.so.1.26.0",
            ];

            for path in candidates {
                if std::path::Path::new(path).exists() {
                    ort::init_from(path)?;
                    ort::init().commit();
                    return Ok(());
                }
            }

            // Dynamic discovery via python3 — works for any distro or pip --user install.
            if let Ok(output) = std::process::Command::new("python3")
                .args([
                    "-c",
                    "import onnxruntime as o, os; \
                     capi=os.path.join(os.path.dirname(o.__file__),'capi'); \
                     [print(os.path.join(capi,f)) \
                      for f in os.listdir(capi) \
                      if f.startswith('libonnxruntime') and '.so' in f]",
                ])
                .output()
            {
                if output.status.success() {
                    for line in String::from_utf8_lossy(&output.stdout).lines() {
                        let p = line.trim();
                        if !p.is_empty() && std::path::Path::new(p).exists() {
                            ort::init_from(p)?;
                            ort::init().commit();
                            return Ok(());
                        }
                    }
                }
            }

            // Last resort: let ort try standard dlopen lookup.
            ort::init().commit();
            Ok(())
        };
        try_init().map_err(|e| e.to_string())
    });
    result.as_ref().map(|_| ()).map_err(|e| anyhow::anyhow!("{e}"))
}

// ── Kokoro ONNX inference ─────────────────────────────────────────────────────

/// Run a single Kokoro synthesis pass and return raw f32 audio samples.
fn run_kokoro_inference(
    session: &mut ort::session::Session,
    tokens: &[i64],
    style: &[f32],
    speed: f32,
) -> Result<Vec<f32>> {
    use ort::value::Tensor;

    let t = tokens.len();
    let tokens_tensor = Tensor::<i64>::from_array(([1i64, t as i64], tokens.to_vec()))
        .context("build tokens tensor")?;
    let style_tensor = Tensor::<f32>::from_array(([1i64, 256i64], style.to_vec()))
        .context("build style tensor")?;
    let speed_tensor = Tensor::<f32>::from_array(([1i64], vec![speed]))
        .context("build speed tensor")?;

    let has_input_ids = session.inputs().iter().any(|i| i.name() == "input_ids");
    let id_name: &str = if has_input_ids { "input_ids" } else { "tokens" };

    // Build named input list as Vec<(String, SessionInputValue)>
    let inputs: Vec<(String, ort::session::SessionInputValue<'_>)> = vec![
        (id_name.to_string(), tokens_tensor.into()),
        ("style".to_string(), style_tensor.into()),
        ("speed".to_string(), speed_tensor.into()),
    ];

    let outputs = session.run(inputs).context("Kokoro ONNX inference")?;

    let (_, audio_slice) = outputs[0]
        .try_extract_tensor::<f32>()
        .context("extract audio tensor from Kokoro output")?;

    Ok(audio_slice.to_vec())
}

// ── Kokoro data layout ────────────────────────────────────────────────────────

pub fn kokoro_data_dir(data_dir: &str) -> PathBuf {
    if data_dir.is_empty() {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("kokoro")
    } else {
        expand_tilde(data_dir)
    }
}

fn kokoro_model_filename(quality: &str) -> &'static str {
    match quality {
        "fp16" => "kokoro-v1.0.fp16.onnx",
        "int8" => "kokoro-v1.0.int8.onnx",
        _ => "kokoro-v1.0.onnx",
    }
}

fn kokoro_model_url(quality: &str) -> String {
    let filename = kokoro_model_filename(quality);
    format!(
        "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/{filename}"
    )
}

const KOKORO_VOICES_URL: &str =
    "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin";

/// True when both the selected model file and the voices directory with voices are present on disk.
pub fn is_kokoro_ready(quality: &str, data_dir: &str) -> bool {
    let dir = kokoro_data_dir(data_dir);
    dir.join(kokoro_model_filename(quality)).exists() && dir.join("voices").join("af_heart.npy").exists()
}

// ── Kokoro download ───────────────────────────────────────────────────────────

fn extract_voices_zip(zip_path: &Path, dest_dir: &Path) -> Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;
    std::fs::create_dir_all(dest_dir)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.ends_with(".npy") {
            let out_path = dest_dir.join(name);
            let mut out_file = std::fs::File::create(&out_path)?;
            std::io::copy(&mut entry, &mut out_file)?;
        }
    }
    Ok(())
}

pub async fn download_kokoro_assets(quality: &str, data_dir: &str) -> Result<()> {
    let dir = kokoro_data_dir(data_dir);
    tokio::fs::create_dir_all(&dir).await?;

    let model_filename = kokoro_model_filename(quality);
    let model_path = dir.join(model_filename);
    if !model_path.exists() {
        let url = kokoro_model_url(quality);
        info!("Downloading Kokoro model ({quality}): {url}");
        download_file(&url, &model_path).await?;
        info!("Kokoro model saved to {}", model_path.display());
    } else {
        info!("Kokoro model already present: {}", model_path.display());
    }

    let voices_zip_path = dir.join("voices-v1.0.bin");
    let voices_dir = dir.join("voices");

    if !voices_dir.join("af_heart.npy").exists() {
        if !voices_zip_path.exists() {
            info!("Downloading Kokoro voices pack: {KOKORO_VOICES_URL}");
            download_file(KOKORO_VOICES_URL, &voices_zip_path).await?;
            info!("Kokoro voices saved to {}", voices_zip_path.display());
        }

        info!("Extracting Kokoro voices ZIP to {}...", voices_dir.display());
        extract_voices_zip(&voices_zip_path, &voices_dir)?;

        if voices_zip_path.exists() {
            let _ = std::fs::remove_file(&voices_zip_path);
            info!("Deleted voices ZIP archive to free space.");
        }
    } else {
        info!("Kokoro unzipped voices already present in {}", voices_dir.display());
        if voices_zip_path.exists() {
            let _ = std::fs::remove_file(&voices_zip_path);
        }
    }

    info!("Kokoro assets ready in {}", dir.display());
    Ok(())
}

async fn download_file(url: &str, dest: &Path) -> Result<()> {
    let response = reqwest::get(url).await?.error_for_status()?;
    let bytes = response.bytes().await?;
    let tmp = tempfile::NamedTempFile::new_in(dest.parent().unwrap_or(Path::new(".")))?;
    std::io::copy(&mut bytes.as_ref(), &mut tmp.as_file())?;
    tmp.persist(dest)?;
    Ok(())
}

// ── Piper helpers ─────────────────────────────────────────────────────────────

pub fn piper_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("piper-voices")
}

fn expand_tilde(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn resolve_voices_dir(voice_dir: &str) -> PathBuf {
    if voice_dir.is_empty() {
        piper_voices_dir()
    } else {
        expand_tilde(voice_dir)
    }
}

pub fn piper_binary() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "piper.exe" } else { "piper" };
    let local = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("piper")
        .join(exe);
    if local.exists() {
        return Some(local);
    }
    voxctrl_config::find_in_path("piper")
}

// ── Utterance queue ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,
    pub source_label: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TtsCommand {
    Play {
        utterance: Utterance,
        generation: u32,
    },
    Shutdown,
}

static ACTIVE_SINK: std::sync::Mutex<Option<std::sync::Arc<rodio::Sink>>> = std::sync::Mutex::new(None);

pub fn stop_current_playback() {
    let mut guard = ACTIVE_SINK.lock().unwrap();
    if let Some(ref sink) = *guard {
        let _ = sink.stop();
    }
    *guard = None;
}

#[derive(Clone)]
pub struct TtsEngineHandle {
    tx: Sender<TtsCommand>,
    generation: Arc<std::sync::atomic::AtomicU32>,
}

impl TtsEngineHandle {
    pub fn speak(&self, text: impl Into<String>) {
        let gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(TtsCommand::Play {
            utterance: Utterance {
                text: text.into(),
                voice: None,
                source_label: None,
            },
            generation: gen,
        });
    }

    pub fn speak_utterance(&self, u: Utterance) {
        let gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(TtsCommand::Play {
            utterance: u,
            generation: gen,
        });
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        stop_current_playback();
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(TtsCommand::Shutdown);
    }
}

// ── TTS engine worker ─────────────────────────────────────────────────────────

pub type PlaybackCallback = Arc<dyn Fn() + Send + Sync + 'static>;

pub struct TtsEngineWorker {
    config: TtsConfig,
    rx: Receiver<TtsCommand>,
    generation: Arc<std::sync::atomic::AtomicU32>,
    on_playback_start: Option<PlaybackCallback>,
    on_playback_end: Option<PlaybackCallback>,
}

impl TtsEngineWorker {
    pub fn start(
        config: TtsConfig,
        on_playback_start: Option<PlaybackCallback>,
        on_playback_end: Option<PlaybackCallback>,
    ) -> TtsEngineHandle {
        let (tx, rx) = bounded(32);
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handle = TtsEngineHandle { tx, generation: generation.clone() };

        if config.engine == TtsEngine::Kokoro && config.kokoro.prewarm {
            let _ = handle.tx.send(TtsCommand::Play {
                utterance: Utterance {
                    text: " ".into(),
                    voice: None,
                    source_label: Some("prewarm".into()),
                },
                generation: 0,
            });
        }

        let worker = Self { config, rx, generation, on_playback_start, on_playback_end };
        std::thread::Builder::new()
            .name("voxctrl-tts".into())
            .spawn(move || worker.run())
            .expect("spawn tts thread");

        handle
    }

    fn run(self) {
        info!("TTS engine started (engine={:?})", self.config.engine);
        // Kokoro ONNX session cached for the lifetime of this worker thread.
        let mut kokoro_session: Option<ort::session::Session> = None;

        // Persistent Rodio Output Stream - kept alive for the lifetime of this thread!
        let mut audio_context: Option<(rodio::OutputStream, rodio::OutputStreamHandle, Arc<rodio::Sink>)> = None;

        let init_audio = |ctx: &mut Option<(rodio::OutputStream, rodio::OutputStreamHandle, Arc<rodio::Sink>)>| -> Result<Arc<rodio::Sink>> {
            if let Some((_, _, ref sink)) = ctx {
                return Ok(sink.clone());
            }
            let (stream, handle) = rodio::OutputStream::try_default()
                .map_err(|e| anyhow::anyhow!("audio output device: {e}"))?;
            let sink = Arc::new(rodio::Sink::try_new(&handle)
                .map_err(|e| anyhow::anyhow!("audio sink: {e}"))?);
            *ctx = Some((stream, handle, sink.clone()));
            Ok(sink)
        };

        while let Ok(cmd) = self.rx.recv() {
            match cmd {
                TtsCommand::Play { utterance, generation } => {
                    let current_gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
                    if generation < current_gen {
                        debug!("Discarding stale utterance: generation={generation} (current={current_gen})");
                        continue;
                    }

                    let is_prewarm = utterance.source_label.as_deref() == Some("prewarm");

                    let sink_res = init_audio(&mut audio_context);
                    if let Err(e) = sink_res {
                        warn!("TTS audio init error: {e}");
                        continue;
                    }
                    let sink = sink_res.unwrap();

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = Some(sink.clone());
                    }

                    let result = match self.config.engine {
                        TtsEngine::Piper => self.speak_piper(&utterance, &sink),
                        TtsEngine::Espeak => self.speak_espeak(&utterance),
                        TtsEngine::Kokoro => {
                            speak_kokoro(&self.config, &utterance, &mut kokoro_session, &self.on_playback_start, &sink)
                        }
                    };

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = None;
                    }

                    if let Err(e) = result {
                        warn!("TTS speak error: {e}");
                    }
                    if !is_prewarm {
                        if let Some(ref cb) = self.on_playback_end {
                            cb();
                        }
                    }
                }
                TtsCommand::Shutdown => {
                    debug!("TTS shutdown signal received");
                    stop_current_playback();
                    break;
                }
            }
        }
    }

    fn speak_piper(&self, u: &Utterance, sink: &rodio::Sink) -> Result<()> {
        let binary = piper_binary().context("piper binary not found")?;
        let voice_name = u.voice.as_deref().unwrap_or(&self.config.voice);

        let voice_path =
            get_voice_path(voice_name, &self.config.voice_dir).ok_or_else(|| {
                anyhow::anyhow!("Piper voice files not found for: {}", voice_name)
            })?;

        let length_scale = 1.0 / self.config.speed;
        let mut cmd = std::process::Command::new(&binary);
        cmd.arg("--model")
            .arg(&voice_path)
            .arg("--length-scale")
            .arg(length_scale.to_string())
            .arg("--output-raw");

        if self.config.gpu {
            cmd.arg("--cuda");
        }

        let mut piper = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn piper")?;

        use std::io::Write;
        piper
            .stdin
            .as_mut()
            .unwrap()
            .write_all(u.text.as_bytes())
            .context("write to piper stdin")?;

        let output = piper.wait_with_output().context("wait piper")?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "piper process failed with exit code {:?}: {}",
                output.status.code(),
                err_msg.trim()
            );
        }

        if output.stdout.is_empty() {
            anyhow::bail!("piper produced empty stdout");
        }

        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        play_raw_audio(sink, &output.stdout, sample_rate_for_voice(voice_name))?;
        Ok(())
    }

    fn speak_espeak(&self, u: &Utterance) -> Result<()> {
        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        let wpm = (175.0 * self.config.speed) as i32;
        std::process::Command::new("espeak-ng")
            .arg("-s")
            .arg(wpm.to_string())
            .arg(&u.text)
            .status()
            .context("espeak-ng")?;
        Ok(())
    }
}

// ── Kokoro synthesis (pure Rust / ONNX) ──────────────────────────────────────

fn speak_kokoro(
    config: &TtsConfig,
    u: &Utterance,
    session: &mut Option<ort::session::Session>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
) -> Result<()> {
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");

    let dir = kokoro_data_dir(&config.kokoro.data_dir);
    let model_path = dir.join(kokoro_model_filename(&config.kokoro.quality));
    let voices_dir = dir.join("voices");

    if !model_path.exists() {
        anyhow::bail!(
            "Kokoro model not found at {}. Download it from TTS settings.",
            model_path.display()
        );
    }
    if !voices_dir.join("af_heart.npy").exists() {
        anyhow::bail!("Kokoro voices folder not found or incomplete. Download it from TTS settings.");
    }

    // Lazily load the ONNX session — stays alive for the worker thread lifetime.
    if session.is_none() {
        ensure_ort_init().context("initialise ONNX Runtime")?;
        info!("Loading Kokoro ONNX session: {} (gpu={})", model_path.display(), config.gpu);
        let mut sb = ort::session::Session::builder()
            .context("ONNX session builder")?;

        if config.gpu {
            sb = match sb.with_execution_providers([ort::execution_providers::CUDAExecutionProvider::default().build()]) {
                Ok(builder) => {
                    info!("Successfully registered CUDA Execution Provider for Kokoro");
                    builder
                }
                Err(e) => {
                    warn!("Failed to register CUDA Execution Provider: {e}. Falling back to CPU.");
                    match ort::session::Session::builder() {
                        Ok(fallback_sb) => fallback_sb,
                        Err(fallback_err) => {
                            warn!("Failed to create fallback ONNX session builder: {fallback_err}");
                            return Err(anyhow::anyhow!("fallback builder failure: {fallback_err}"));
                        }
                    }
                }
            };
        }

        *session = Some(
            sb.commit_from_file(&model_path)
                .context("load Kokoro ONNX model")?
        );
    }
    let sess = session.as_mut().unwrap();

    let voice = u.voice.as_deref().unwrap_or(&config.kokoro.voice);
    let speed = config.speed;
    let lang = if voice.starts_with('b') { "en-gb" } else { "en-us" };

    let phonemes = phonemize_espeak(&u.text, lang)?;
    if phonemes.is_empty() {
        return Ok(());
    }

    let (tokens, num_inner) = kokoro_tokenize(&phonemes);
    let style = load_voice_embedding(&voices_dir, voice, num_inner)?;
    let audio = run_kokoro_inference(sess, &tokens, &style, speed)?;

    if is_prewarm {
        return Ok(());
    }

    if let Some(ref cb) = on_playback_start {
        cb();
    }

    // Convert f32 samples → i16 PCM bytes for rodio playback.
    let bytes: Vec<u8> = audio
        .iter()
        .flat_map(|&s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();

    play_raw_audio(sink, &bytes, KOKORO_SAMPLE_RATE)
}

// ── Audio playback ────────────────────────────────────────────────────────────

fn play_raw_audio(sink: &rodio::Sink, raw: &[u8], sample_rate: u32) -> Result<()> {
    let samples: Vec<i16> = raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
    sink.sleep_until_end();
    Ok(())
}

// ── Voice catalogue helpers ───────────────────────────────────────────────────

fn voice_name_to_filename(name: &str) -> Option<String> {
    PIPER_VOICES
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.filename.to_string())
}

fn sample_rate_for_voice(name: &str) -> u32 {
    PIPER_VOICES
        .iter()
        .find(|v| v.name == name)
        .map(|v| v.sample_rate)
        .unwrap_or(22050)
}

pub fn is_voice_downloaded(voice_name: &str, voice_dir: &str) -> bool {
    get_voice_path(voice_name, voice_dir).is_some()
}

// ── Piper voice download ──────────────────────────────────────────────────────

const PIPER_RELEASE_BASE: &str =
    "https://github.com/rhasspy/piper/releases/download/v0.0.2/";

pub fn get_voice_path(voice_name: &str, voice_dir: &str) -> Option<PathBuf> {
    let filename = voice_name_to_filename(voice_name)
        .unwrap_or_else(|| format!("{voice_name}.onnx"));

    let voices_dir = resolve_voices_dir(voice_dir);

    let path_onnx = voices_dir.join(&filename);
    let path_json = voices_dir.join(format!("{filename}.json"));
    if path_onnx.exists() && path_json.exists() {
        return Some(path_onnx);
    }

    let filename_lower = filename.to_lowercase();
    let path_onnx_lower = voices_dir.join(&filename_lower);
    let path_json_lower = voices_dir.join(format!("{filename_lower}.json"));
    if path_onnx_lower.exists() && path_json_lower.exists() {
        return Some(path_onnx_lower);
    }

    let path_raw_lower = voices_dir.join(format!("{}.onnx", voice_name.to_lowercase()));
    let path_raw_json_lower =
        voices_dir.join(format!("{}.onnx.json", voice_name.to_lowercase()));
    if path_raw_lower.exists() && path_raw_json_lower.exists() {
        return Some(path_raw_lower);
    }

    None
}

pub async fn download_piper_binary() -> Result<()> {
    #[cfg(unix)]
    {
        let local_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("piper");

        tokio::fs::create_dir_all(&local_dir).await?;

        let dest_exe = local_dir.join("piper");
        if dest_exe.exists() {
            return Ok(());
        }

        info!("Downloading standalone Piper binary...");
        let url =
            "https://github.com/rhasspy/piper/releases/download/v1.2.0/piper_amd64.tar.gz";

        let response = reqwest::get(url).await?.error_for_status()?;
        let bytes = response.bytes().await?;

        info!("Extracting Piper binary...");
        let cursor = std::io::Cursor::new(bytes);
        let tar = flate2::read::GzDecoder::new(cursor);
        let mut archive = tar::Archive::new(tar);
        for entry in archive.entries()? {
            let mut entry = entry?;
            let path = entry.path()?.into_owned();
            if let Some(file_name) = path.file_name() {
                let dest = local_dir.join(file_name);
                let mut outfile = std::fs::File::create(&dest)?;
                std::io::copy(&mut entry, &mut outfile)?;

                use std::os::unix::fs::PermissionsExt;
                if let Ok(metadata) = std::fs::metadata(&dest) {
                    let mut perms = metadata.permissions();
                    perms.set_mode(0o755);
                    let _ = std::fs::set_permissions(&dest, perms);
                }
            }
        }
        info!("Standalone Piper binary installed to {}", dest_exe.display());
    }
    Ok(())
}

pub async fn download_voice(voice_name: &str, voice_dir: &str) -> Result<()> {
    if piper_binary().is_none() {
        if let Err(e) = download_piper_binary().await {
            warn!("Failed to download standalone piper binary: {e}");
        }
    }

    let voices_dir = resolve_voices_dir(voice_dir);
    tokio::fs::create_dir_all(&voices_dir).await?;

    if get_voice_path(voice_name, voice_dir).is_some() {
        info!("Voice {} is already downloaded.", voice_name);
        return Ok(());
    }

    let tarball_url = format!("{PIPER_RELEASE_BASE}voice-{voice_name}.tar.gz");
    info!("Downloading voice tarball: {tarball_url}");

    let response = reqwest::get(&tarball_url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    info!("Extracting voice files...");
    let cursor = std::io::Cursor::new(bytes);
    let tar = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let file_name = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        if file_name.ends_with(".onnx") || file_name.ends_with(".onnx.json") {
            let dest_path = voices_dir.join(&file_name);
            let mut temp_file = tempfile::NamedTempFile::new_in(&voices_dir)?;
            std::io::copy(&mut entry, &mut temp_file)?;
            temp_file.persist(&dest_path)?;
            info!("Extracted: {}", dest_path.display());
        }
    }

    info!("Voice files successfully downloaded and extracted.");
    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn create_fake_voice(dir: &std::path::Path, filename: &str) {
        fs::write(dir.join(filename), b"fake onnx model").unwrap();
        fs::write(dir.join(format!("{filename}.json")), b"{}").unwrap();
    }

    // ── Helpers for NPY test data ──────────────────────────────────────

    fn make_npy_data(rows: usize, cols: usize) -> Vec<u8> {
        let header_str = format!(
            "{{'descr': '<f4', 'fortran_order': False, 'shape': ({rows}, {cols}), }}"
        );
        // Pad header so that total header block (magic 6 + ver 2 + len 2 + header) is a multiple of 64.
        let prefix_len = 10usize; // magic(6) + major(1) + minor(1) + header_len(2)
        let raw_len = prefix_len + header_str.len() + 1; // +1 for trailing \n
        let pad = (64 - raw_len % 64) % 64;
        let mut header = header_str;
        for _ in 0..pad {
            header.push(' ');
        }
        header.push('\n');

        let mut out = Vec::new();
        out.extend_from_slice(b"\x93NUMPY");
        out.push(1u8); // major
        out.push(0u8); // minor
        out.extend_from_slice(&(header.len() as u16).to_le_bytes());
        out.extend_from_slice(header.as_bytes());
        for i in 0..(rows * cols) {
            out.extend_from_slice(&(i as f32).to_le_bytes());
        }
        out
    }

    fn write_npy_file(dir: &std::path::Path, voice: &str, rows: usize, cols: usize) {
        let npy = make_npy_data(rows, cols);
        fs::write(dir.join(format!("{voice}.npy")), npy).unwrap();
    }

    // ── resolve_voices_dir ────────────────────────────────────────────────────

    #[test]
    fn test_resolve_voices_dir_empty_uses_default() {
        let result = resolve_voices_dir("");
        assert_eq!(result, piper_voices_dir());
    }

    #[test]
    fn test_resolve_voices_dir_absolute_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let result = resolve_voices_dir(path);
        assert_eq!(result, dir.path());
    }

    #[test]
    fn test_resolve_voices_dir_tilde_expands() {
        let result = resolve_voices_dir("~/my-voices");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home.join("my-voices"));
    }

    #[test]
    fn test_resolve_voices_dir_tilde_alone_expands() {
        let result = resolve_voices_dir("~");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home);
    }

    // ── expand_tilde ──────────────────────────────────────────────────────────

    #[test]
    fn test_expand_tilde_home() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn test_expand_tilde_subdir() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(expand_tilde("~/.piper-voices"), home.join(".piper-voices"));
    }

    #[test]
    fn test_expand_tilde_absolute_unchanged() {
        assert_eq!(expand_tilde("/usr/share/voices"), PathBuf::from("/usr/share/voices"));
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("relative/path"), PathBuf::from("relative/path"));
    }

    // ── is_voice_downloaded ───────────────────────────────────────────────────

    #[test]
    fn test_is_voice_downloaded_default_dir_not_present() {
        let _ = is_voice_downloaded("en-us-lessac-medium", "");
    }

    #[test]
    fn test_is_voice_downloaded_returns_true_when_files_exist() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        create_fake_voice(dir.path(), "en_US-amy-low.onnx");
        assert!(is_voice_downloaded("en-us-amy-low", path));
    }

    #[test]
    fn test_is_voice_downloaded_returns_false_when_files_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(!is_voice_downloaded("en-us-amy-low", path));
    }

    #[test]
    fn test_is_voice_downloaded_returns_false_for_nonexistent_dir() {
        assert!(!is_voice_downloaded("en-us-amy-low", "/nonexistent/path/xyz"));
    }

    #[test]
    fn test_is_voice_downloaded_tilde_path() {
        let _ = is_voice_downloaded("en-us-lessac-medium", "~/.local/share/voxctrl/piper-voices");
    }

    #[test]
    fn test_is_voice_downloaded_only_onnx_not_sufficient() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("en_US-amy-low.onnx"), b"fake").unwrap();
        assert!(!is_voice_downloaded("en-us-amy-low", path));
    }

    // ── get_voice_path ────────────────────────────────────────────────────────

    #[test]
    fn test_get_voice_path_returns_none_when_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(get_voice_path("en-us-ryan-high", path).is_none());
    }

    #[test]
    fn test_get_voice_path_returns_some_when_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        create_fake_voice(dir.path(), "en_US-ryan-high.onnx");
        let result = get_voice_path("en-us-ryan-high", path);
        assert!(result.is_some());
        assert!(result.unwrap().exists());
    }

    #[test]
    fn test_get_voice_path_accepts_custom_dir() {
        let dir = tempdir().unwrap();
        let other_dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let other_path = other_dir.path().to_str().unwrap();
        create_fake_voice(other_dir.path(), "en_US-danny-low.onnx");
        assert!(get_voice_path("en-us-danny-low", path).is_none());
        assert!(get_voice_path("en-us-danny-low", other_path).is_some());
    }

    #[test]
    fn test_get_voice_path_lowercase_fallback() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let lc_name = "en_us-lessac-medium.onnx";
        fs::write(dir.path().join(lc_name), b"fake").unwrap();
        fs::write(dir.path().join(format!("{lc_name}.json")), b"{}").unwrap();
        assert!(get_voice_path("en-us-lessac-medium", path).is_some());
    }

    // ── Piper voice catalogue ─────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_not_empty() {
        assert!(!PIPER_VOICES.is_empty());
    }

    #[test]
    fn test_piper_voices_have_required_fields() {
        for v in PIPER_VOICES {
            assert!(!v.name.is_empty());
            assert!(!v.quality.is_empty());
            assert!(!v.filename.is_empty());
            assert!(v.sample_rate > 0);
        }
    }

    #[test]
    fn test_piper_voices_names_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in PIPER_VOICES {
            assert!(seen.insert(v.name), "duplicate piper voice name: {}", v.name);
        }
    }

    #[test]
    fn test_piper_voices_filenames_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in PIPER_VOICES {
            assert!(seen.insert(v.filename), "duplicate piper filename: {}", v.filename);
        }
    }

    #[test]
    fn test_piper_voices_quality_values_are_valid() {
        let valid = ["high", "medium", "low"];
        for v in PIPER_VOICES {
            assert!(valid.contains(&v.quality), "unexpected quality '{}' for {}", v.quality, v.name);
        }
    }

    #[test]
    fn test_piper_voices_sample_rates_are_valid() {
        let valid_rates = [16000u32, 22050u32];
        for v in PIPER_VOICES {
            assert!(valid_rates.contains(&v.sample_rate), "unexpected sample_rate {} for {}", v.sample_rate, v.name);
        }
    }

    #[test]
    fn test_piper_voices_filenames_end_with_onnx() {
        for v in PIPER_VOICES {
            assert!(v.filename.ends_with(".onnx"), "filename should end with .onnx: {}", v.filename);
        }
    }

    // ── sample_rate_for_voice ─────────────────────────────────────────────────

    #[test]
    fn test_sample_rate_for_known_high_quality_voice() {
        assert_eq!(sample_rate_for_voice("en-us-ryan-high"), 22050);
    }

    #[test]
    fn test_sample_rate_for_known_low_quality_voice() {
        assert_eq!(sample_rate_for_voice("en-us-amy-low"), 16000);
    }

    #[test]
    fn test_sample_rate_for_unknown_voice_defaults_to_22050() {
        assert_eq!(sample_rate_for_voice("xx-unknown-voice"), 22050);
    }

    // ── piper_binary ──────────────────────────────────────────────────────────

    #[test]
    fn test_piper_binary_returns_option_without_panicking() {
        let _ = piper_binary();
    }

    // ── piper_voices_dir ──────────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_dir_not_empty() {
        let d = piper_voices_dir();
        assert!(d.components().count() > 0);
    }

    #[test]
    fn test_piper_voices_dir_ends_with_piper_voices() {
        let d = piper_voices_dir();
        assert!(d.ends_with("voxctrl/piper-voices"));
    }

    // ── voice_name_to_filename ────────────────────────────────────────────────

    #[test]
    fn test_voice_name_to_filename_known() {
        assert_eq!(
            voice_name_to_filename("en-us-lessac-medium"),
            Some("en_US-lessac-medium.onnx".to_string())
        );
    }

    #[test]
    fn test_voice_name_to_filename_unknown_returns_none() {
        assert_eq!(voice_name_to_filename("xx-unknown-voice"), None);
    }

    #[test]
    fn test_voice_name_to_filename_all_piper_voices_resolve() {
        for v in PIPER_VOICES {
            let result = voice_name_to_filename(v.name);
            assert!(result.is_some(), "voice_name_to_filename should resolve {}", v.name);
            assert_eq!(result.unwrap(), v.filename);
        }
    }

    // ── Kokoro voice catalogue ────────────────────────────────────────────────

    #[test]
    fn test_kokoro_voices_not_empty() {
        assert!(!KOKORO_VOICES.is_empty());
    }

    #[test]
    fn test_kokoro_voices_have_required_fields() {
        for v in KOKORO_VOICES {
            assert!(!v.id.is_empty());
            assert!(!v.label.is_empty());
            assert!(!v.lang.is_empty());
        }
    }

    #[test]
    fn test_kokoro_voices_ids_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in KOKORO_VOICES {
            assert!(seen.insert(v.id), "duplicate voice id: {}", v.id);
        }
    }

    #[test]
    fn test_kokoro_voices_cover_expected_prefixes() {
        let ids: Vec<&str> = KOKORO_VOICES.iter().map(|v| v.id).collect();
        assert!(ids.iter().any(|id| id.starts_with("af_")), "missing American female voices");
        assert!(ids.iter().any(|id| id.starts_with("am_")), "missing American male voices");
        assert!(ids.iter().any(|id| id.starts_with("bf_")), "missing British female voices");
        assert!(ids.iter().any(|id| id.starts_with("bm_")), "missing British male voices");
    }

    #[test]
    fn test_kokoro_voices_lang_matches_prefix() {
        for v in KOKORO_VOICES {
            if v.id.starts_with('a') {
                assert_eq!(v.lang, "en-us", "American voice {} should have lang en-us", v.id);
            } else if v.id.starts_with('b') {
                assert_eq!(v.lang, "en-gb", "British voice {} should have lang en-gb", v.id);
            }
        }
    }

    // ── Kokoro vocabulary ─────────────────────────────────────────────────────

    #[test]
    fn test_kokoro_vocab_size() {
        assert_eq!(kokoro_vocab().len(), 114);
    }

    #[test]
    fn test_kokoro_vocab_contains_key_ipa_symbols() {
        let vocab = kokoro_vocab();
        assert_eq!(vocab.get(&'ə'), Some(&83u32));  // schwa
        assert_eq!(vocab.get(&'\u{02C8}'), Some(&156u32)); // primary stress ˈ
        assert_eq!(vocab.get(&'\u{02D0}'), Some(&158u32)); // long vowel ː
        assert_eq!(vocab.get(&' '), Some(&16u32));  // space
        assert_eq!(vocab.get(&'h'), Some(&50u32));
        assert_eq!(vocab.get(&'l'), Some(&54u32));
    }

    #[test]
    fn test_kokoro_vocab_no_duplicate_ids() {
        let mut seen = std::collections::HashSet::new();
        for &(_, id) in KOKORO_VOCAB_PAIRS {
            assert!(seen.insert(id), "duplicate token ID {id} in KOKORO_VOCAB_PAIRS");
        }
    }

    #[test]
    fn test_kokoro_vocab_no_duplicate_chars() {
        let mut seen = std::collections::HashSet::new();
        for &(ch, _) in KOKORO_VOCAB_PAIRS {
            assert!(seen.insert(ch), "duplicate char {:?} in KOKORO_VOCAB_PAIRS", ch);
        }
    }

    // ── kokoro_tokenize ───────────────────────────────────────────────────────

    #[test]
    fn test_kokoro_tokenize_empty_phonemes() {
        let (tokens, num_inner) = kokoro_tokenize("");
        assert_eq!(tokens, vec![0i64, 0i64]);
        assert_eq!(num_inner, 0);
    }

    #[test]
    fn test_kokoro_tokenize_all_unknown_chars() {
        // Characters not in vocab map to nothing; only boundary 0s remain.
        let (tokens, num_inner) = kokoro_tokenize("\x01\x02\x03");
        assert_eq!(tokens, vec![0i64, 0i64]);
        assert_eq!(num_inner, 0);
    }

    #[test]
    fn test_kokoro_tokenize_known_phonemes() {
        // "həl" → [0, 83(ə→wait, h=50), 83, 54, 0]
        let (tokens, num_inner) = kokoro_tokenize("həl");
        assert_eq!(tokens, vec![0, 50, 83, 54, 0]);
        assert_eq!(num_inner, 3);
    }

    #[test]
    fn test_kokoro_tokenize_produces_boundary_zeros() {
        let (tokens, _) = kokoro_tokenize("h");
        assert_eq!(tokens[0], 0, "first token should be boundary 0");
        assert_eq!(*tokens.last().unwrap(), 0, "last token should be boundary 0");
    }

    #[test]
    fn test_kokoro_tokenize_truncates_at_max() {
        // Input of MAX_PHONEME_LENGTH+10 'h' chars should be capped.
        let long_input = "h".repeat(MAX_PHONEME_LENGTH + 10);
        let (tokens, num_inner) = kokoro_tokenize(&long_input);
        assert_eq!(num_inner, MAX_PHONEME_LENGTH);
        assert_eq!(tokens.len(), MAX_PHONEME_LENGTH + 2); // inner + 2 boundary
    }

    #[test]
    fn test_kokoro_tokenize_hello_world() {
        // Verify the known tokenisation of "həlˈoʊ wˈɜːld"
        let phonemes = "həl\u{02C8}o\u{028A} w\u{02C8}\u{025C}\u{02D0}ld";
        let (tokens, num_inner) = kokoro_tokenize(phonemes);
        // Expected: [0, h=50, ə=83, l=54, ˈ=156, o=57, ʊ=135, ' '=16, w=65, ˈ=156, ɜ=87, ː=158, l=54, d=46, 0]
        assert_eq!(tokens, vec![0, 50, 83, 54, 156, 57, 135, 16, 65, 156, 87, 158, 54, 46, 0]);
        assert_eq!(num_inner, 13);
    }

    // ── kokoro_data_dir ───────────────────────────────────────────────────────

    #[test]
    fn test_kokoro_data_dir_empty_uses_default() {
        let result = kokoro_data_dir("");
        assert!(result.ends_with("voxctrl/kokoro"));
    }

    #[test]
    fn test_kokoro_data_dir_custom_path() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert_eq!(kokoro_data_dir(path), dir.path());
    }

    #[test]
    fn test_kokoro_data_dir_tilde_expands() {
        let result = kokoro_data_dir("~/my-kokoro");
        let home = dirs::home_dir().unwrap();
        assert_eq!(result, home.join("my-kokoro"));
    }

    // ── kokoro_model_filename ─────────────────────────────────────────────────

    #[test]
    fn test_kokoro_model_filename_f32() {
        assert_eq!(kokoro_model_filename("f32"), "kokoro-v1.0.onnx");
    }

    #[test]
    fn test_kokoro_model_filename_fp16() {
        assert_eq!(kokoro_model_filename("fp16"), "kokoro-v1.0.fp16.onnx");
    }

    #[test]
    fn test_kokoro_model_filename_int8() {
        assert_eq!(kokoro_model_filename("int8"), "kokoro-v1.0.int8.onnx");
    }

    #[test]
    fn test_kokoro_model_filename_unknown_falls_back_to_f32() {
        assert_eq!(kokoro_model_filename("unknown"), "kokoro-v1.0.onnx");
    }

    // ── is_kokoro_ready ───────────────────────────────────────────────────────

    #[test]
    fn test_is_kokoro_ready_false_when_files_missing() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        assert!(!is_kokoro_ready("fp16", path));
    }

    #[test]
    fn test_is_kokoro_ready_true_when_both_files_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("kokoro-v1.0.fp16.onnx"), b"fake model").unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        fs::write(voices_dir.join("af_heart.npy"), b"fake voices").unwrap();
        assert!(is_kokoro_ready("fp16", path));
    }

    #[test]
    fn test_is_kokoro_ready_false_when_only_model_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("kokoro-v1.0.onnx"), b"fake model").unwrap();
        assert!(!is_kokoro_ready("f32", path));
    }

    #[test]
    fn test_is_kokoro_ready_false_when_only_voices_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        fs::write(voices_dir.join("af_heart.npy"), b"fake voices").unwrap();
        assert!(!is_kokoro_ready("f32", path));
    }

    #[test]
    fn test_is_kokoro_ready_checks_correct_model_for_quality() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        fs::write(voices_dir.join("af_heart.npy"), b"fake").unwrap();
        fs::write(dir.path().join("kokoro-v1.0.fp16.onnx"), b"fake").unwrap();
        assert!(is_kokoro_ready("fp16", path));
        assert!(!is_kokoro_ready("f32", path));
        assert!(!is_kokoro_ready("int8", path));
    }

    // ── load_voice_embedding (NPY) ─────────────────────────────────────

    #[test]
    fn test_load_voice_embedding_invalid_magic_returns_error() {
        let dir = tempdir().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        let bad_npy = b"NOTANPY\x01\x00\x00";
        fs::write(voices_dir.join("af_heart_bad.npy"), bad_npy).unwrap();

        let result = load_voice_embedding(&voices_dir, "af_heart_bad", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_voice_embedding_missing_voice_returns_error() {
        let dir = tempdir().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        write_npy_file(&voices_dir, "af_heart", 10, 256);

        // Requesting a voice not in the directory should error.
        let result = load_voice_embedding(&voices_dir, "af_bella", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_voice_embedding_returns_256_floats() {
        let dir = tempdir().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        write_npy_file(&voices_dir, "af_heart", 20, 256);

        let result = load_voice_embedding(&voices_dir, "af_heart", 5).unwrap();
        assert_eq!(result.len(), 256);
    }

    #[test]
    fn test_load_voice_embedding_row_indexing() {
        let dir = tempdir().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        write_npy_file(&voices_dir, "af_heart", 10, 256);

        let row0 = load_voice_embedding(&voices_dir, "af_heart", 0).unwrap();
        let row1 = load_voice_embedding(&voices_dir, "af_heart", 1).unwrap();
        // Row 0 starts at float 0.0; row 1 starts at float 256.0
        assert!((row0[0] - 0.0f32).abs() < f32::EPSILON);
        assert!((row1[0] - 256.0f32).abs() < f32::EPSILON);
    }

    #[test]
    fn test_load_voice_embedding_clamps_out_of_bounds_row() {
        let dir = tempdir().unwrap();
        let voices_dir = dir.path().join("voices");
        fs::create_dir_all(&voices_dir).unwrap();
        write_npy_file(&voices_dir, "af_heart", 5, 256);

        // Row 9999 should clamp to last valid row without panicking.
        let result = load_voice_embedding(&voices_dir, "af_heart", 9999);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 256);
    }
}

// ── FIFO response listener ────────────────────────────────────────────────────

pub async fn run_fifo_responder(fifo_path: String, tts: TtsEngineHandle) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    info!("FIFO responder watching {fifo_path}");
    loop {
        while !std::path::Path::new(&fifo_path).exists() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        match tokio::fs::File::open(&fifo_path).await {
            Ok(file) => {
                let mut lines = BufReader::new(file).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim().to_string();
                    if !line.is_empty() {
                        tts.speak(line);
                    }
                }
            }
            Err(e) => {
                warn!("FIFO open error {fifo_path}: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
