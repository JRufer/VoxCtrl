use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use tracing::{debug, info, warn};
use voxctrl_config::{TtsConfig, TtsEngine};

// ── Voice catalogue ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub quality: &'static str,
    pub sample_rate: u32,
    pub filename: &'static str,
}

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

/// Piper voice catalogue
pub static PIPER_VOICES: &[VoiceInfo] = &[
    VoiceInfo { name: "en-us-libritts-high",   quality: "high",    sample_rate: 22050, filename: "en_US-libritts-high.onnx" },
    VoiceInfo { name: "en-us-amy-low",         quality: "low",     sample_rate: 16000, filename: "en_US-amy-low.onnx" },
    VoiceInfo { name: "en-us-kathleen-low",    quality: "low",     sample_rate: 16000, filename: "en_US-kathleen-low.onnx" },
    VoiceInfo { name: "en-gb-southern_english_female-low", quality: "low", sample_rate: 16000, filename: "en_GB-southern_english_female-low.onnx" },
    VoiceInfo { name: "en-us-ryan-high",       quality: "high",    sample_rate: 22050, filename: "en_US-ryan-high.onnx" },
    VoiceInfo { name: "en-us-ryan-medium",     quality: "medium",  sample_rate: 22050, filename: "en_US-ryan-medium.onnx" },
    VoiceInfo { name: "en-us-ryan-low",        quality: "low",     sample_rate: 16000, filename: "en_US-ryan-low.onnx" },
    VoiceInfo { name: "en-us-lessac-medium",   quality: "medium",  sample_rate: 16000, filename: "en_US-lessac-medium.onnx" },
    VoiceInfo { name: "en-us-lessac-low",      quality: "low",     sample_rate: 16000, filename: "en_US-lessac-low.onnx" },
    VoiceInfo { name: "en-us-danny-low",       quality: "low",     sample_rate: 16000, filename: "en_US-danny-low.onnx" },
    VoiceInfo { name: "en-gb-alan-low",        quality: "low",     sample_rate: 16000, filename: "en_GB-alan-low.onnx" },
];

pub fn is_voice_downloaded(voice_name: &str, voice_dir: &str) -> bool {
    get_voice_path(voice_name, voice_dir).is_some()
}

// ── Kokoro data layout ────────────────────────────────────────────────────────

/// Return the path to VoxCtrl's Kokoro data directory.
/// `data_dir` overrides the default when non-empty.
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

/// True when both the selected model file and the voices pack exist.
pub fn is_kokoro_ready(quality: &str, data_dir: &str) -> bool {
    let dir = kokoro_data_dir(data_dir);
    dir.join(kokoro_model_filename(quality)).exists() && dir.join("voices-v1.0.bin").exists()
}

// ── Kokoro Python helper script ───────────────────────────────────────────────

const KOKORO_HELPER_SCRIPT: &str = r#"#!/usr/bin/env python3
"""VoxCtrl kokoro-onnx synthesis helper.
Usage: python3 synthesize.py <model_path> <voices_path> <voice_id> <speed> <output_wav>
Text is read from stdin.
Requires: pip install kokoro-onnx
"""
import sys
import wave

model_path  = sys.argv[1]
voices_path = sys.argv[2]
voice       = sys.argv[3]
speed       = float(sys.argv[4])
output_path = sys.argv[5]

text = sys.stdin.buffer.read().decode("utf-8").strip()
if not text:
    sys.exit(0)

try:
    from kokoro_onnx import Kokoro
except ImportError:
    print(
        "ERROR: kokoro-onnx is not installed.\n"
        "Install it with: pip install kokoro-onnx",
        file=sys.stderr,
    )
    sys.exit(1)

import numpy as np

# Derive lang_code from the voice prefix (a=American, b=British)
lang_code = "b" if voice.startswith("b") else "a"

kokoro = Kokoro(model_path, voices_path)
samples, sample_rate = kokoro.create(
    text, voice=voice, speed=speed, lang_code=lang_code
)

# Write WAV using only stdlib (no soundfile required)
samples_i16 = (np.clip(samples, -1.0, 1.0) * 32767.0).astype(np.int16)
with wave.open(output_path, "w") as wf:
    wf.setnchannels(1)
    wf.setsampwidth(2)
    wf.setframerate(sample_rate)
    wf.writeframes(samples_i16.tobytes())
"#;

/// Write the helper script to the Kokoro data directory (idempotent).
pub fn ensure_kokoro_script(data_dir: &str) -> Result<PathBuf> {
    let dir = kokoro_data_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let script_path = dir.join("synthesize.py");
    std::fs::write(&script_path, KOKORO_HELPER_SCRIPT)?;
    Ok(script_path)
}

/// Find a usable Python 3 interpreter.
pub fn find_python3() -> Option<PathBuf> {
    for name in &["python3", "python"] {
        if let Some(p) = voxctrl_config::find_in_path(name) {
            return Some(p);
        }
    }
    None
}

// ── Kokoro download ───────────────────────────────────────────────────────────

/// Download the Kokoro ONNX model and voices pack to the data directory.
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

    let voices_path = dir.join("voices-v1.0.bin");
    if !voices_path.exists() {
        info!("Downloading Kokoro voices pack: {KOKORO_VOICES_URL}");
        download_file(KOKORO_VOICES_URL, &voices_path).await?;
        info!("Kokoro voices saved to {}", voices_path.display());
    } else {
        info!("Kokoro voices already present: {}", voices_path.display());
    }

    ensure_kokoro_script(data_dir)?;
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

#[derive(Clone)]
pub struct TtsEngineHandle {
    tx: Sender<Option<Utterance>>,
}

impl TtsEngineHandle {
    pub fn speak(&self, text: impl Into<String>) {
        let _ = self.tx.send(Some(Utterance {
            text: text.into(),
            voice: None,
            source_label: None,
        }));
    }

    pub fn speak_utterance(&self, u: Utterance) {
        let _ = self.tx.send(Some(u));
    }

    pub fn stop(&self) {
        let _ = self.tx.send(None);
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct TtsEngineWorker {
    config: TtsConfig,
    rx: Receiver<Option<Utterance>>,
}

impl TtsEngineWorker {
    pub fn start(config: TtsConfig) -> TtsEngineHandle {
        let (tx, rx) = bounded(32);
        let handle = TtsEngineHandle { tx };

        // Queue a prewarm utterance so the worker pre-loads the model on startup.
        if config.engine == TtsEngine::Kokoro && config.kokoro.prewarm {
            let _ = handle.tx.send(Some(Utterance {
                text: " ".into(),
                voice: None,
                source_label: Some("prewarm".into()),
            }));
        }

        let worker = Self { config, rx };
        std::thread::Builder::new()
            .name("voxctrl-tts".into())
            .spawn(move || worker.run())
            .expect("spawn tts thread");

        handle
    }

    fn run(self) {
        info!("TTS engine started (engine={:?})", self.config.engine);
        while let Ok(msg) = self.rx.recv() {
            match msg {
                Some(utterance) => {
                    if let Err(e) = self.speak_one(&utterance) {
                        warn!("TTS speak error: {e}");
                    }
                }
                None => {
                    debug!("TTS stop/shutdown signal received");
                    break;
                }
            }
        }
    }

    fn speak_one(&self, u: &Utterance) -> Result<()> {
        match self.config.engine {
            TtsEngine::Piper => self.speak_piper(u),
            TtsEngine::Espeak => self.speak_espeak(u),
            TtsEngine::Kokoro => self.speak_kokoro(u),
        }
    }

    fn speak_piper(&self, u: &Utterance) -> Result<()> {
        let binary = piper_binary().context("piper binary not found")?;
        let voice_name = u
            .voice
            .as_deref()
            .unwrap_or(&self.config.voice);

        let voice_path = get_voice_path(voice_name, &self.config.voice_dir).ok_or_else(|| {
            anyhow::anyhow!("Piper voice files not found for: {}", voice_name)
        })?;

        // piper reads from stdin, produces raw PCM on stdout; played via rodio
        let mut piper = std::process::Command::new(&binary)
            .arg("--model")
            .arg(&voice_path)
            .arg("--output-raw")
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

        play_raw_audio(&output.stdout, sample_rate_for_voice(voice_name))?;
        Ok(())
    }

    fn speak_espeak(&self, u: &Utterance) -> Result<()> {
        std::process::Command::new("espeak-ng")
            .arg(&u.text)
            .status()
            .context("espeak-ng")?;
        Ok(())
    }

    fn speak_kokoro(&self, u: &Utterance) -> Result<()> {
        let is_prewarm = u.source_label.as_deref() == Some("prewarm");

        let python = find_python3().context(
            "Python 3 not found in PATH. Install Python 3 and run: pip install kokoro-onnx",
        )?;

        let data_dir = &self.config.kokoro.data_dir;
        let quality = &self.config.kokoro.quality;
        let script_path = ensure_kokoro_script(data_dir).context("write kokoro helper script")?;

        let dir = kokoro_data_dir(data_dir);
        let model_path = dir.join(kokoro_model_filename(quality));
        let voices_path = dir.join("voices-v1.0.bin");

        if !model_path.exists() {
            anyhow::bail!(
                "Kokoro model not found at {}. Download it from the TTS settings.",
                model_path.display()
            );
        }
        if !voices_path.exists() {
            anyhow::bail!("Kokoro voices-v1.0.bin not found. Download it from the TTS settings.");
        }

        let voice = u.voice.as_deref().unwrap_or(&self.config.kokoro.voice);
        let speed = self.config.kokoro.speed;

        let output_path = std::env::temp_dir().join(format!(
            "voxctrl_kokoro_{}.wav",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));

        use std::io::Write;
        let mut child = std::process::Command::new(&python)
            .arg(&script_path)
            .arg(&model_path)
            .arg(&voices_path)
            .arg(voice)
            .arg(speed.to_string())
            .arg(&output_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn kokoro helper")?;

        child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(u.text.as_bytes())
            .context("write text to kokoro stdin")?;

        let result = child.wait_with_output().context("wait kokoro")?;
        if !result.status.success() {
            let err = String::from_utf8_lossy(&result.stderr);
            let _ = std::fs::remove_file(&output_path);
            anyhow::bail!("kokoro synthesis failed: {}", err.trim());
        }

        if !is_prewarm && output_path.exists() {
            let play_result = play_wav_file(&output_path);
            let _ = std::fs::remove_file(&output_path);
            return play_result;
        }

        let _ = std::fs::remove_file(&output_path);
        Ok(())
    }
}

fn play_wav_file(path: &Path) -> Result<()> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("open wav file: {}", path.display()))?;
    let (_stream, handle) = rodio::OutputStream::try_default()
        .map_err(|e| anyhow::anyhow!("audio output device: {e}"))?;
    let sink = rodio::Sink::try_new(&handle)
        .map_err(|e| anyhow::anyhow!("audio sink: {e}"))?;
    let decoder = rodio::Decoder::new(std::io::BufReader::new(file))
        .map_err(|e| anyhow::anyhow!("wav decode: {e}"))?;
    sink.append(decoder);
    sink.sleep_until_end();
    Ok(())
}

fn play_raw_audio(raw: &[u8], sample_rate: u32) -> Result<()> {
    let samples: Vec<i16> = raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    let (_stream, handle) = rodio::OutputStream::try_default()
        .map_err(|e| anyhow::anyhow!("audio output device: {e}"))?;
    let sink = rodio::Sink::try_new(&handle)
        .map_err(|e| anyhow::anyhow!("audio sink: {e}"))?;
    sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
    sink.sleep_until_end();
    Ok(())
}

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

// ── Voice download ────────────────────────────────────────────────────────────

const PIPER_RELEASE_BASE: &str =
    "https://github.com/rhasspy/piper/releases/download/v0.0.2/";

pub fn get_voice_path(voice_name: &str, voice_dir: &str) -> Option<PathBuf> {
    let filename = voice_name_to_filename(voice_name)
        .unwrap_or_else(|| format!("{voice_name}.onnx"));

    let voices_dir = resolve_voices_dir(voice_dir);

    // Check exact case
    let path_onnx = voices_dir.join(&filename);
    let path_json = voices_dir.join(format!("{filename}.json"));
    if path_onnx.exists() && path_json.exists() {
        return Some(path_onnx);
    }

    // Check lowercase version of exact filename
    let filename_lower = filename.to_lowercase();
    let path_onnx_lower = voices_dir.join(&filename_lower);
    let path_json_lower = voices_dir.join(format!("{filename_lower}.json"));
    if path_onnx_lower.exists() && path_json_lower.exists() {
        return Some(path_onnx_lower);
    }

    // Check raw name lowercase fallback
    let path_raw_lower = voices_dir.join(format!("{}.onnx", voice_name.to_lowercase()));
    let path_raw_json_lower = voices_dir.join(format!("{}.onnx.json", voice_name.to_lowercase()));
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
        let url = "https://github.com/rhasspy/piper/releases/download/v1.2.0/piper_amd64.tar.gz";

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
    // If the local standalone piper binary is missing, download it first
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
        let result = expand_tilde("/usr/share/voices");
        assert_eq!(result, std::path::PathBuf::from("/usr/share/voices"));
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        let result = expand_tilde("relative/path");
        assert_eq!(result, std::path::PathBuf::from("relative/path"));
    }

    // ── is_voice_downloaded ───────────────────────────────────────────────────

    #[test]
    fn test_is_voice_downloaded_default_dir_not_present() {
        // Uses the real default dir; the voice is unlikely to be present in CI.
        // The call must not panic; the return value is environment-dependent.
        let _ = is_voice_downloaded("en-us-lessac-medium", "");
    }

    #[test]
    fn test_is_voice_downloaded_returns_true_when_files_exist() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        // en-us-amy-low maps to en_US-amy-low.onnx
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
        // Tilde-prefixed path must not panic; result depends on whether the
        // home dir contains voice files (unlikely in CI so we just check no panic).
        let _ = is_voice_downloaded("en-us-lessac-medium", "~/.local/share/voxctrl/piper-voices");
    }

    #[test]
    fn test_is_voice_downloaded_only_onnx_not_sufficient() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        // Write only the .onnx, omit the .json — should still return false
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

        // Put voice in `other_dir`, not in `dir`
        create_fake_voice(other_dir.path(), "en_US-danny-low.onnx");

        assert!(get_voice_path("en-us-danny-low", path).is_none());
        assert!(get_voice_path("en-us-danny-low", other_path).is_some());
    }

    #[test]
    fn test_get_voice_path_lowercase_fallback() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        // Write file with fully lowercase name
        let lc_name = "en_us-lessac-medium.onnx";
        fs::write(dir.path().join(lc_name), b"fake").unwrap();
        fs::write(dir.path().join(format!("{lc_name}.json")), b"{}").unwrap();
        // Should be found via the lowercase fallback branch
        let result = get_voice_path("en-us-lessac-medium", path);
        assert!(result.is_some());
    }

    // ── Piper voice catalogue ─────────────────────────────────────────────────

    #[test]
    fn test_piper_voices_not_empty() {
        assert!(!PIPER_VOICES.is_empty());
    }

    #[test]
    fn test_piper_voices_have_required_fields() {
        for v in PIPER_VOICES {
            assert!(!v.name.is_empty(), "name must not be empty");
            assert!(!v.quality.is_empty(), "quality must not be empty");
            assert!(!v.filename.is_empty(), "filename must not be empty");
            assert!(v.sample_rate > 0, "sample_rate must be > 0");
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
            assert!(
                valid.contains(&v.quality),
                "unexpected quality '{}' for {}",
                v.quality,
                v.name
            );
        }
    }

    #[test]
    fn test_piper_voices_sample_rates_are_valid() {
        let valid_rates = [16000u32, 22050u32];
        for v in PIPER_VOICES {
            assert!(
                valid_rates.contains(&v.sample_rate),
                "unexpected sample_rate {} for {}",
                v.sample_rate,
                v.name
            );
        }
    }

    #[test]
    fn test_piper_voices_filenames_end_with_onnx() {
        for v in PIPER_VOICES {
            assert!(
                v.filename.ends_with(".onnx"),
                "piper voice filename should end with .onnx, got: {}",
                v.filename
            );
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
        // Whether piper is installed or not, this must not panic.
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
            assert!(
                result.is_some(),
                "voice_name_to_filename should resolve {}",
                v.name
            );
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
            assert!(!v.id.is_empty(), "voice id must not be empty");
            assert!(!v.label.is_empty(), "voice label must not be empty");
            assert!(!v.lang.is_empty(), "voice lang must not be empty");
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
        let has_af = ids.iter().any(|id| id.starts_with("af_"));
        let has_am = ids.iter().any(|id| id.starts_with("am_"));
        let has_bf = ids.iter().any(|id| id.starts_with("bf_"));
        let has_bm = ids.iter().any(|id| id.starts_with("bm_"));
        assert!(has_af, "should have American female voices");
        assert!(has_am, "should have American male voices");
        assert!(has_bf, "should have British female voices");
        assert!(has_bm, "should have British male voices");
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
        fs::write(dir.path().join("voices-v1.0.bin"), b"fake voices").unwrap();
        assert!(is_kokoro_ready("fp16", path));
    }

    #[test]
    fn test_is_kokoro_ready_false_when_only_model_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("kokoro-v1.0.onnx"), b"fake model").unwrap();
        // voices-v1.0.bin is missing
        assert!(!is_kokoro_ready("f32", path));
    }

    #[test]
    fn test_is_kokoro_ready_false_when_only_voices_present() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("voices-v1.0.bin"), b"fake voices").unwrap();
        // model is missing
        assert!(!is_kokoro_ready("f32", path));
    }

    #[test]
    fn test_is_kokoro_ready_checks_correct_model_for_quality() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        fs::write(dir.path().join("voices-v1.0.bin"), b"fake voices").unwrap();

        // Only fp16 present – f32 and int8 should still be false
        fs::write(dir.path().join("kokoro-v1.0.fp16.onnx"), b"fake").unwrap();
        assert!(is_kokoro_ready("fp16", path));
        assert!(!is_kokoro_ready("f32", path));
        assert!(!is_kokoro_ready("int8", path));
    }

    // ── ensure_kokoro_script ──────────────────────────────────────────────────

    #[test]
    fn test_ensure_kokoro_script_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let script = ensure_kokoro_script(path).unwrap();
        assert!(script.exists(), "helper script should be written to disk");
        assert_eq!(script.file_name().unwrap(), "synthesize.py");
    }

    #[test]
    fn test_ensure_kokoro_script_contains_kokoro_import() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let script = ensure_kokoro_script(path).unwrap();
        let content = fs::read_to_string(&script).unwrap();
        assert!(content.contains("from kokoro_onnx import Kokoro"));
        assert!(content.contains("kokoro.create"));
    }

    #[test]
    fn test_ensure_kokoro_script_idempotent() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let s1 = ensure_kokoro_script(path).unwrap();
        let s2 = ensure_kokoro_script(path).unwrap();
        assert_eq!(s1, s2);
        assert_eq!(
            fs::read_to_string(&s1).unwrap(),
            fs::read_to_string(&s2).unwrap()
        );
    }
}

// ── FIFO response listener ────────────────────────────────────────────────────

/// Watches a FIFO for newline-delimited text and speaks each line via TTS.
pub async fn run_fifo_responder(fifo_path: String, tts: TtsEngineHandle) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    info!("FIFO responder watching {fifo_path}");
    loop {
        // Wait until the FIFO exists (agent may start later)
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
                // EOF: agent disconnected; re-open
            }
            Err(e) => {
                warn!("FIFO open error {fifo_path}: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
