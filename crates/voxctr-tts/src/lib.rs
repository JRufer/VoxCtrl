use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
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

pub fn piper_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctl")
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
        .join("voxctl")
        .join("piper")
        .join(exe);
    if local.exists() {
        return Some(local);
    }
    voxctr_config::find_in_path("piper")
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
            TtsEngine::Piper => {
                match self.speak_piper(u) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        warn!("Piper TTS failed or not found: {e}. Falling back to Espeak-ng...");
                        self.speak_espeak(u)
                    }
                }
            }
            TtsEngine::Espeak => self.speak_espeak(u),
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

pub async fn download_voice(voice_name: &str, voice_dir: &str) -> Result<()> {
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
        let _ = is_voice_downloaded("en-us-lessac-medium", "~/.local/share/voxctl/piper-voices");
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
