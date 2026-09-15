//! Breeze-TTS-2 neural text-to-speech engine support, via the shared
//! audio.cpp runtime (see `audiocpp.rs`).
//!
//! Model repository: <https://huggingface.co/BreezeBlue/Breeze-TTS-2>
//! Gated model weights released under the BreezeBlue Research and
//! Non-Commercial License — see `audiocpp::BREEZE_TTS_2_LICENSE_NOTE` and
//! the Settings/setup-wizard warnings before download.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::TtsConfig;

use crate::audiocpp::{
    self, resolve_hf_reference, resolve_hf_reference_blocking, resolve_model_dir, synthesize,
    SpeakerRef, SynthesizeRequest,
};
use crate::engine::{PlaybackCallback, Utterance};
use crate::pocket::resolve_wav_reference_clip;

pub const BREEZE_TTS_2_SAMPLE_RATE: u32 = 24_000;
const BREEZE_TTS_2_GGUF_REPO: &str = "audio-cpp/audio.cpp-gguf";
const BREEZE_TTS_2_GGUF_FILE: &str = "Breeze-TTS-2-GGUF/breeze-tts-2-q8_0.gguf";
const BREEZE_TTS_2_MODEL_FILENAME: &str = "breeze-tts-2-q8_0.gguf";

/// Surfaced by Settings and the setup wizard before a Breeze-TTS-2 download.
pub const BREEZE_TTS_2_LICENSE_NOTE: &str =
    "Breeze-TTS-2 model weights are released under the BreezeBlue Research and \
     Non-Commercial License. Commercial use requires a separate license from the \
     model's publisher.";

pub fn breeze_tts_2_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("breeze-tts-2")
}

pub fn resolve_breeze_tts_2_dir(model_dir: &str) -> PathBuf {
    resolve_model_dir(model_dir, breeze_tts_2_model_dir)
}

pub fn is_breeze_tts_2_ready(model_dir: &str) -> bool {
    resolve_breeze_tts_2_dir(model_dir).join(BREEZE_TTS_2_MODEL_FILENAME).exists()
}

/// Downloads the Breeze-TTS-2 GGUF model into `model_dir`. The audio.cpp GGUF
/// mirror is not gated, so `hf_token` is accepted for parity with the other
/// engines but not required.
pub async fn download_breeze_tts_2_assets(model_dir: &str, hf_token: Option<String>) -> Result<()> {
    if audiocpp::audiocpp_binary().is_none() {
        audiocpp::download_audiocpp_binary().await.context("download audio.cpp runtime")?;
    }

    let dir = resolve_breeze_tts_2_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create breeze-tts-2 model dir {}", dir.display()))?;

    let dest = dir.join(BREEZE_TTS_2_MODEL_FILENAME);
    if !dest.exists() {
        info!("Downloading Breeze-TTS-2 model ({BREEZE_TTS_2_GGUF_FILE})...");
        let downloaded = resolve_hf_reference(
            &format!("hf://{BREEZE_TTS_2_GGUF_REPO}/{BREEZE_TTS_2_GGUF_FILE}"),
            hf_token.as_deref(),
        )
        .await
        .context("download breeze-tts-2 model")?;
        tokio::fs::copy(&downloaded, &dest).await.context("place breeze-tts-2 model")?;
    }

    info!("Breeze-TTS-2 assets ready in {}", dir.display());
    Ok(())
}

fn read_voice_transcript_file(wav_path: &std::path::Path) -> Option<String> {
    let txt_path = wav_path.with_extension("txt");
    let content = std::fs::read_to_string(&txt_path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        info!("Loaded voice transcript for {:?}: '{}'", txt_path.file_name(), trimmed);
        Some(trimmed)
    }
}

/// Called from `TtsEngineWorker::run` when `config.engine ==
/// TtsEngine::BreezeTts2`.
pub(crate) fn speak_breeze_tts_2(
    config: &TtsConfig,
    u: &Utterance,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    _generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    _generation: u32,
) -> Result<()> {
    let cfg = &config.breeze_tts_2;
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let model_dir = resolve_breeze_tts_2_dir(&cfg.model_dir);
    if !model_dir.join(BREEZE_TTS_2_MODEL_FILENAME).exists() {
        anyhow::bail!("Breeze-TTS-2 model not found. Download it from TTS settings.");
    }

    let is_clone_mode =
        cfg.voice_mode == "clone" || (!cfg.cloned_voice.trim().is_empty() && cfg.voice_mode != "prompt");

    let (clip_path, transcript) = if is_clone_mode {
        let voice_id = if cfg.cloned_voice.trim().is_empty() { "alba" } else { cfg.cloned_voice.trim() };
        let reference = resolve_wav_reference_clip(voice_id, &cfg.voice_dir)
            .unwrap_or_else(|| "hf://kyutai/tts-voices/alba-mackenna/casual.wav".to_string());
        let path = resolve_hf_reference_blocking(&reference, config.hf_token.as_deref())
            .context("resolve Breeze-TTS-2 reference voice clip")?;
        // Breeze-TTS-2 cloning requires a matching transcript, unlike
        // Pocket-TTS/VoxCPM2 — audio.cpp rejects a clone request with no
        // `--reference-text` for this family.
        let transcript = read_voice_transcript_file(&path).ok_or_else(|| {
            anyhow::anyhow!(
                "Breeze-TTS-2 voice cloning needs a transcript: add a {}.txt file \
                 next to the reference clip containing exactly what is spoken in it.",
                path.file_stem().and_then(|s| s.to_str()).unwrap_or("<voice>")
            )
        })?;
        (Some(path), Some(transcript))
    } else {
        (None, None)
    };

    if is_prewarm {
        return Ok(());
    }

    let out_wav = tempfile::Builder::new()
        .prefix("voxctrl-breeze-tts-2-")
        .suffix(".wav")
        .tempfile()
        .context("create temp wav for breeze-tts-2")?;

    let speaker = match &clip_path {
        Some(path) => SpeakerRef::Clone(path),
        None => SpeakerRef::Design(cfg.speaker_prompt.trim()),
    };

    info!(
        "Synthesizing text with Breeze-TTS-2 (mode={}, gpu={})...",
        if clip_path.is_some() { "clone" } else { "design" },
        cfg.gpu
    );

    synthesize(
        &SynthesizeRequest {
            family: audiocpp::FAMILY_BREEZE_TTS,
            model_dir: &model_dir,
            gpu: cfg.gpu,
            text: &u.text,
            speaker: Some(speaker),
            reference_text: transcript.as_deref(),
            load_options: &[],
        },
        out_wav.path(),
    )?;

    if let Some(ref cb) = on_playback_start {
        cb();
    }
    audiocpp::play_wav_file(sink, out_wav.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_breeze_tts_2_model_dir() {
        assert!(breeze_tts_2_model_dir().ends_with("breeze-tts-2"));
    }

    #[test]
    fn test_resolve_breeze_tts_2_dir() {
        assert_eq!(resolve_breeze_tts_2_dir(""), breeze_tts_2_model_dir());
        assert_eq!(resolve_breeze_tts_2_dir("/tmp/breeze"), PathBuf::from("/tmp/breeze"));
    }

    #[test]
    fn test_is_breeze_tts_2_ready_false_when_empty() {
        let dir = tempdir().unwrap();
        assert!(!is_breeze_tts_2_ready(dir.path().to_str().unwrap()));
    }
}
