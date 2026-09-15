//! VoxCPM2 neural text-to-speech engine support, via the shared audio.cpp
//! runtime (see `audiocpp.rs`).
//!
//! Model repository: <https://huggingface.co/openbmb/VoxCPM2>
//! Open-source model weights released under the Apache-2.0 License.
//!
//! VoxCPM2 supports Voice Design (natural-language prompts) and Voice Cloning
//! (reference .wav clips), including Ultimate Cloning with a paired
//! audio + transcript for higher-fidelity cloning.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::TtsConfig;

use crate::audiocpp::{
    self, resolve_hf_reference, resolve_hf_reference_blocking, resolve_model_dir, AudioCppSession,
    SpeakRequest, SpeakerRef,
};
use crate::engine::{PlaybackCallback, Utterance};
use crate::pocket::resolve_wav_reference_clip;

pub const VOX_CPM_2_SAMPLE_RATE: u32 = 24_000;
const VOX_CPM_2_GGUF_REPO: &str = "audio-cpp/audio.cpp-gguf";
const VOX_CPM_2_GGUF_FILE: &str = "VoxCPM2-GGUF/voxcpm2-q8_0.gguf";
const VOX_CPM_2_MODEL_FILENAME: &str = "voxcpm2-q8_0.gguf";

pub fn vox_cpm_2_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("voxcpm2")
}

pub fn resolve_vox_cpm_2_dir(model_dir: &str) -> PathBuf {
    resolve_model_dir(model_dir, vox_cpm_2_model_dir)
}

pub fn is_vox_cpm_2_ready(model_dir: &str) -> bool {
    resolve_vox_cpm_2_dir(model_dir).join(VOX_CPM_2_MODEL_FILENAME).exists()
}

/// Downloads the VoxCPM2 GGUF model into `model_dir`. Ungated, so `hf_token`
/// is accepted for parity with the other engines but not required.
pub async fn download_vox_cpm_2_assets(model_dir: &str, hf_token: Option<String>) -> Result<()> {
    if audiocpp::audiocpp_binary().is_none() {
        audiocpp::download_audiocpp_binary().await.context("download audio.cpp runtime")?;
    }

    let dir = resolve_vox_cpm_2_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create voxcpm2 model dir {}", dir.display()))?;

    let dest = dir.join(VOX_CPM_2_MODEL_FILENAME);
    if !dest.exists() {
        info!("Downloading VoxCPM2 model ({VOX_CPM_2_GGUF_FILE})...");
        let downloaded = resolve_hf_reference(
            &format!("hf://{VOX_CPM_2_GGUF_REPO}/{VOX_CPM_2_GGUF_FILE}"),
            hf_token.as_deref(),
        )
        .await
        .context("download voxcpm2 model")?;
        tokio::fs::copy(&downloaded, &dest).await.context("place voxcpm2 model")?;
    }

    info!("VoxCPM2 assets ready in {}", dir.display());
    Ok(())
}

pub fn read_voice_transcript_file(wav_path_str: &str) -> Option<String> {
    let txt_path = std::path::Path::new(wav_path_str).with_extension("txt");
    let content = std::fs::read_to_string(&txt_path).ok()?;
    let trimmed = content.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        info!("Loaded voice transcript for {:?}: '{}'", txt_path.file_name(), trimmed);
        Some(trimmed)
    }
}

/// Resolves the clone-mode reference clip + optional transcript, or `None`
/// when the config calls for Voice Design instead (which the UI no longer
/// offers for VoxCPM2 — see the module docs above — but a config predating
/// that change could still have it set).
fn resolve_clone_reference(
    cfg: &voxctrl_config::VoxCpm2Config,
    hf_token: Option<&str>,
) -> Result<Option<(std::path::PathBuf, Option<String>)>> {
    let is_clone_mode =
        cfg.voice_mode == "clone" || (!cfg.cloned_voice.trim().is_empty() && cfg.voice_mode != "prompt");
    if !is_clone_mode {
        return Ok(None);
    }

    let voice_id = if cfg.cloned_voice.trim().is_empty() { "alba" } else { cfg.cloned_voice.trim() };
    let reference = resolve_wav_reference_clip(voice_id, &cfg.voice_dir)
        .unwrap_or_else(|| "hf://kyutai/tts-voices/alba-mackenna/casual.wav".to_string());
    let path =
        resolve_hf_reference_blocking(&reference, hf_token).context("resolve VoxCPM2 reference voice clip")?;
    let transcript = read_voice_transcript_file(&path.to_string_lossy());
    if cfg.ultimate_cloning {
        match &transcript {
            Some(t) => info!("VoxCPM2 Ultimate Cloning active with transcript: '{t}'"),
            None => info!("VoxCPM2 Ultimate Cloning enabled; no companion .txt found alongside the reference clip"),
        }
    }
    Ok(Some((path, transcript)))
}

fn speaker_ref<'a>(
    cfg: &'a voxctrl_config::VoxCpm2Config,
    clone_ref: &'a Option<(std::path::PathBuf, Option<String>)>,
) -> SpeakerRef<'a> {
    match clone_ref {
        Some((path, _)) => SpeakerRef::Clone(path),
        None => {
            // Confirmed against audio.cpp v0.8.0: `instruct` is accepted for
            // voxcpm2's `tts` task but produces byte-identical output
            // regardless of prompt content — Voice Design isn't actually
            // wired up for this family yet, despite being advertised in its
            // model spec. Not something VoxCtrl can work around; flagging it
            // loudly here so it isn't mistaken for our own bug.
            tracing::warn!(
                "VoxCPM2 Voice Design prompt is currently ignored by audio.cpp \
                 (a known upstream limitation) — synthesis will use its default voice"
            );
            SpeakerRef::Design(cfg.speaker_prompt.trim())
        }
    }
}

/// Ensures a resident audio.cpp session is loaded and warm for VoxCPM2.
/// Called from `TtsCommand::Preload` — see [`crate::pocket::ensure_pocket_tts_loaded`]
/// for why this needs its own dummy request rather than reusing `speak_vox_cpm_2`.
pub(crate) fn ensure_vox_cpm_2_loaded(
    config: &TtsConfig,
    session: &mut Option<AudioCppSession>,
) -> Result<()> {
    let cfg = &config.vox_cpm_2;
    let model_dir = resolve_vox_cpm_2_dir(&cfg.model_dir);
    if !model_dir.join(VOX_CPM_2_MODEL_FILENAME).exists() {
        anyhow::bail!("VoxCPM2 model not found. Download it from TTS settings.");
    }
    AudioCppSession::ensure(session, audiocpp::FAMILY_VOXCPM2, &model_dir, cfg.gpu)?;

    let clone_ref = resolve_clone_reference(cfg, config.hf_token.as_deref())?;
    let reference_text = if cfg.ultimate_cloning { clone_ref.as_ref().and_then(|(_, t)| t.as_deref()) } else { None };
    session.as_ref().unwrap().speak(&SpeakRequest {
        text: " ",
        speaker: Some(speaker_ref(cfg, &clone_ref)),
        reference_text,
    })?;
    Ok(())
}

/// Called from `TtsEngineWorker::run` when `config.engine ==
/// TtsEngine::VoxCpm2`. Takes the worker's audio.cpp session by mutable
/// reference so it persists (and the underlying model stays loaded) across
/// calls for the worker's lifetime, or until idle-unload drops it.
pub(crate) fn speak_vox_cpm_2(
    config: &TtsConfig,
    u: &Utterance,
    session: &mut Option<AudioCppSession>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    _generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    _generation: u32,
) -> Result<()> {
    let cfg = &config.vox_cpm_2;
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let model_dir = resolve_vox_cpm_2_dir(&cfg.model_dir);
    if !model_dir.join(VOX_CPM_2_MODEL_FILENAME).exists() {
        anyhow::bail!("VoxCPM2 model not found. Download it from TTS settings.");
    }
    AudioCppSession::ensure(session, audiocpp::FAMILY_VOXCPM2, &model_dir, cfg.gpu)?;

    let clone_ref = resolve_clone_reference(cfg, config.hf_token.as_deref())?;
    let reference_text = if cfg.ultimate_cloning { clone_ref.as_ref().and_then(|(_, t)| t.as_deref()) } else { None };

    info!(
        "Synthesizing text with VoxCPM2 (mode={}, gpu={})...",
        if clone_ref.is_some() { "clone" } else { "design" },
        cfg.gpu
    );

    let audio = session.as_ref().unwrap().speak(&SpeakRequest {
        text: &u.text,
        speaker: Some(speaker_ref(cfg, &clone_ref)),
        reference_text,
    })?;

    if is_prewarm {
        return Ok(());
    }
    if let Some(ref cb) = on_playback_start {
        cb();
    }
    audiocpp::play_wav_bytes(sink, audio)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_vox_cpm_2_model_dir() {
        assert!(vox_cpm_2_model_dir().ends_with("voxcpm2"));
    }

    #[test]
    fn test_resolve_vox_cpm_2_dir() {
        assert_eq!(resolve_vox_cpm_2_dir(""), vox_cpm_2_model_dir());
        assert_eq!(resolve_vox_cpm_2_dir("/tmp/voxcpm2"), PathBuf::from("/tmp/voxcpm2"));
    }

    #[test]
    fn test_is_vox_cpm_2_ready_false_when_empty() {
        let dir = tempdir().unwrap();
        assert!(!is_vox_cpm_2_ready(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_read_voice_transcript_file() {
        let dir = tempdir().unwrap();
        let wav_path = dir.path().join("my_voice.wav");
        let txt_path = dir.path().join("my_voice.txt");

        assert_eq!(read_voice_transcript_file(wav_path.to_str().unwrap()), None);

        std::fs::write(&wav_path, b"RIFF dummy").unwrap();
        assert_eq!(read_voice_transcript_file(wav_path.to_str().unwrap()), None);

        std::fs::write(&txt_path, "Hello from the transcript!\n").unwrap();
        assert_eq!(
            read_voice_transcript_file(wav_path.to_str().unwrap()),
            Some("Hello from the transcript!".to_string())
        );
    }
}
