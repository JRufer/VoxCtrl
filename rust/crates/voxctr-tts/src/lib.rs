use std::{
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use crossbeam_channel::{bounded, Receiver, Sender};
use tracing::{debug, info, warn};
use voxctr_config::{TtsConfig, TtsEngine};

// ── Voice catalogue ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: &'static str,
    pub quality: &'static str,
    pub sample_rate: u32,
    pub filename: &'static str,
}

pub static PIPER_VOICES: &[VoiceInfo] = &[
    VoiceInfo { name: "en-us-lessac-medium",  quality: "medium",  sample_rate: 22050, filename: "en_US-lessac-medium.onnx" },
    VoiceInfo { name: "en-us-lessac-high",    quality: "high",    sample_rate: 22050, filename: "en_US-lessac-high.onnx" },
    VoiceInfo { name: "en-us-ryan-medium",    quality: "medium",  sample_rate: 22050, filename: "en_US-ryan-medium.onnx" },
    VoiceInfo { name: "en-us-ryan-high",      quality: "high",    sample_rate: 22050, filename: "en_US-ryan-high.onnx" },
    VoiceInfo { name: "en-gb-alan-medium",    quality: "medium",  sample_rate: 22050, filename: "en_GB-alan-medium.onnx" },
    VoiceInfo { name: "en-gb-jenny-dioco-medium", quality: "medium", sample_rate: 22050, filename: "en_GB-jenny_dioco-medium.onnx" },
    VoiceInfo { name: "de-thorsten-medium",   quality: "medium",  sample_rate: 22050, filename: "de_DE-thorsten-medium.onnx" },
    VoiceInfo { name: "fr-upmc-medium",       quality: "medium",  sample_rate: 22050, filename: "fr_FR-upmc-medium.onnx" },
    VoiceInfo { name: "es-carlfm-x-low",      quality: "x_low",   sample_rate: 16000, filename: "es_ES-carlfm-x_low.onnx" },
    VoiceInfo { name: "it-riccardo-x-low",    quality: "x_low",   sample_rate: 16000, filename: "it_IT-riccardo-x_low.onnx" },
];

pub fn piper_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctl")
        .join("piper-voices")
}

pub fn piper_binary() -> Option<PathBuf> {
    // Check local install first, then PATH
    let local = dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctl")
        .join("piper")
        .join("piper");
    if local.exists() {
        return Some(local);
    }
    which_binary("piper").map(PathBuf::from)
}

fn which_binary(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

// ── Utterance queue ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,
    pub source_label: Option<String>,
}

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
        let worker = Self { config, rx };
        std::thread::Builder::new()
            .name("voxctr-tts".into())
            .spawn(move || worker.run())
            .expect("spawn tts thread");
        TtsEngineHandle { tx }
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
                    // Stop signal — kill any current playback (best-effort)
                    debug!("TTS stop signal received");
                }
            }
        }
    }

    fn speak_one(&self, u: &Utterance) -> Result<()> {
        match self.config.engine {
            TtsEngine::Piper => self.speak_piper(u),
            TtsEngine::Espeak => self.speak_espeak(u),
        }
    }

    fn speak_piper(&self, u: &Utterance) -> Result<()> {
        let binary = piper_binary().context("piper binary not found")?;
        let voice_name = u
            .voice
            .as_deref()
            .unwrap_or(&self.config.voice);

        let voice_path = piper_voices_dir().join(
            voice_name_to_filename(voice_name)
                .unwrap_or_else(|| format!("{voice_name}.onnx")),
        );

        if !voice_path.exists() {
            bail!("Piper voice not found: {}", voice_path.display());
        }

        // piper reads from stdin, produces WAV on stdout; pipe to aplay/SoX
        let mut piper = std::process::Command::new(&binary)
            .arg("--model")
            .arg(&voice_path)
            .arg("--output-raw")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
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

        if !output.stdout.is_empty() {
            play_raw_audio(&output.stdout, sample_rate_for_voice(voice_name))?;
        }
        Ok(())
    }

    fn speak_espeak(&self, u: &Utterance) -> Result<()> {
        std::process::Command::new("espeak-ng")
            .arg(&u.text)
            .status()
            .context("espeak-ng")?;
        Ok(())
    }
}

fn play_raw_audio(raw: &[u8], sample_rate: u32) -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        let mut child = std::process::Command::new("aplay")
            .args([
                "-r",
                &sample_rate.to_string(),
                "-f",
                "S16_LE",
                "-c",
                "1",
                "-",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .context("spawn aplay")?;
        use std::io::Write;
        child.stdin.as_mut().unwrap().write_all(raw)?;
        child.wait()?;
    }
    #[cfg(target_os = "windows")]
    {
        // Write to a temp WAV file and play with PowerShell
        let tmp = tempfile::NamedTempFile::with_suffix(".wav")?;
        write_wav(tmp.path(), raw, sample_rate)?;
        std::process::Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "(New-Object Media.SoundPlayer '{}').PlaySync()",
                    tmp.path().display()
                ),
            ])
            .status()
            .context("PowerShell SoundPlayer")?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn write_wav(path: &Path, raw: &[u8], sample_rate: u32) -> Result<()> {
    use std::io::Write;
    let data_len = raw.len() as u32;
    let mut f = std::fs::File::create(path)?;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data_len).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&sample_rate.to_le_bytes())?;
    f.write_all(&(sample_rate * 2).to_le_bytes())?;
    f.write_all(&2u16.to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data_len.to_le_bytes())?;
    f.write_all(raw)?;
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
    "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/";

pub async fn download_voice(voice_name: &str) -> Result<()> {
    let filename = voice_name_to_filename(voice_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown voice: {voice_name}"))?;

    let voices_dir = piper_voices_dir();
    tokio::fs::create_dir_all(&voices_dir).await?;

    let dest = voices_dir.join(&filename);
    if dest.exists() {
        info!("Voice already downloaded: {}", dest.display());
        return Ok(());
    }

    let url = format!("{PIPER_RELEASE_BASE}{filename}");
    info!("Downloading voice: {url}");

    let response = reqwest::get(&url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    // Write atomically via tempfile
    let tmp = tempfile::NamedTempFile::new_in(&voices_dir)?;
    tokio::fs::write(tmp.path(), &bytes).await?;
    tmp.persist(&dest)?;

    info!("Voice downloaded: {}", dest.display());
    Ok(())
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
