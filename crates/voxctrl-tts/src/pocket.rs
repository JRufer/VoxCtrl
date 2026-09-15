//! Pocket-TTS voice catalogue, GGUF asset management, and synthesis via the
//! shared audio.cpp runtime (see `audiocpp.rs`).
//!
//! Named `pocket` rather than `pocket_tts` to avoid colliding with
//! audio.cpp's `pocket_tts` model family name, used as a plain string
//! constant ([`crate::audiocpp::FAMILY_POCKET_TTS`]) rather than a crate.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;

use crate::audiocpp::{self, resolve_hf_reference, synthesize, SpeakerRef, SynthesizeRequest};
use crate::engine::PlaybackCallback;
use crate::piper::expand_tilde;

// ── Pocket-TTS voice catalogue ────────────────────────────────────────────────
//
// audio.cpp's PocketTTS-GGUF package ships precomputed voice embeddings for a
// curated set of named voices (`--voice-id <id>`), so VoxCtrl's built-in
// catalogue just has to name the ones it wants alongside the model — no
// per-voice reference-clip download or on-the-fly cloning needed for these.
// A custom clip dropped into `voice_dir` is still cloned live via
// `--voice-ref`.

#[derive(Debug, Clone)]
pub struct PocketTtsVoiceInfo {
    pub id: &'static str,
    pub label: &'static str,
}

pub static POCKET_TTS_VOICES: &[PocketTtsVoiceInfo] = &[
    PocketTtsVoiceInfo { id: "alba",    label: "Alba (Female)" },
    PocketTtsVoiceInfo { id: "anna",    label: "Anna (Female)" },
    PocketTtsVoiceInfo { id: "vera",    label: "Vera (Female)" },
    PocketTtsVoiceInfo { id: "charles", label: "Charles (Male)" },
    PocketTtsVoiceInfo { id: "michael", label: "Michael (Male)" },
];

pub fn pocket_tts_voice(id: &str) -> Option<&'static PocketTtsVoiceInfo> {
    POCKET_TTS_VOICES.iter().find(|v| v.id == id)
}

/// Default directory scanned for user-supplied voice clips, shared by every
/// voice-cloning engine (Pocket-TTS, Breeze-TTS-2, VoxCPM2) — not specific to
/// Pocket-TTS despite living in this module.
pub fn cloned_tts_voices_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("cloned-tts-voices")
}

fn resolve_cloned_tts_voices_dir(voice_dir: &str) -> PathBuf {
    if voice_dir.is_empty() {
        cloned_tts_voices_dir()
    } else {
        expand_tilde(voice_dir)
    }
}

/// Scans `voice_dir` for `<id>.wav` files, returning `(id, path)` pairs. A file named
/// after a built-in voice (e.g. `alba.wav`) overrides that voice's bundled embedding.
fn scan_custom_pocket_tts_voices(voice_dir: &str) -> Vec<(String, PathBuf)> {
    let dir = resolve_cloned_tts_voices_dir(voice_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else { return Vec::new() };

    let mut found = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()).map(|e| e.eq_ignore_ascii_case("wav")) != Some(true) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        found.push((stem.to_string(), path));
    }
    found
}

fn prettify_voice_label(id: &str) -> String {
    id.replace(['_', '-'], " ")
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct PocketTtsVoiceOption {
    pub id: String,
    pub label: String,
}

/// Built-in voices merged with any custom clips found in `voice_dir`, for the voice picker.
pub fn pocket_tts_voice_catalogue(voice_dir: &str) -> Vec<PocketTtsVoiceOption> {
    let custom = scan_custom_pocket_tts_voices(voice_dir);
    let mut options: Vec<PocketTtsVoiceOption> = POCKET_TTS_VOICES
        .iter()
        .map(|v| PocketTtsVoiceOption { id: v.id.to_string(), label: v.label.to_string() })
        .collect();

    for (id, _) in &custom {
        if let Some(existing) = options.iter_mut().find(|o| &o.id == id) {
            existing.label = format!("{} (Custom)", prettify_voice_label(id));
        } else {
            options.push(PocketTtsVoiceOption {
                id: id.clone(),
                label: format!("{} (Custom)", prettify_voice_label(id)),
            });
        }
    }
    options
}

/// A resolved voice: either a built-in `--voice-id`, or a local clip to clone
/// via `--voice-ref` (a custom drop-in, taking priority over a built-in of the
/// same name).
pub(crate) enum ResolvedVoice {
    BuiltIn(&'static str),
    Custom(PathBuf),
}

pub(crate) fn resolve_pocket_tts_voice(id: &str, voice_dir: &str) -> Option<ResolvedVoice> {
    let custom = scan_custom_pocket_tts_voices(voice_dir);
    if let Some((_, path)) = custom.iter().find(|(custom_id, _)| custom_id == id) {
        return Some(ResolvedVoice::Custom(path.clone()));
    }
    pocket_tts_voice(id).map(|v| ResolvedVoice::BuiltIn(v.id))
}

/// hf:// reference clips for engines that clone from real reference audio
/// (Breeze-TTS-2, VoxCPM2) rather than Pocket-TTS's precomputed named
/// embeddings. Shares voice ids with [`POCKET_TTS_VOICES`] for one consistent
/// picker across all three engines, but resolves to actual audio.
fn builtin_wav_reference_clip(id: &str) -> Option<&'static str> {
    Some(match id {
        "alba" => "hf://kyutai/tts-voices/alba-mackenna/casual.wav",
        "anna" => "hf://kyutai/tts-voices/vctk/p228_023_enhanced.wav",
        "vera" => "hf://kyutai/tts-voices/vctk/p229_023_enhanced.wav",
        "charles" => "hf://kyutai/tts-voices/vctk/p254_023_enhanced.wav",
        "michael" => "hf://kyutai/tts-voices/vctk/p360_023_enhanced.wav",
        _ => return None,
    })
}

/// Resolves a shared voice id to a reference-clip source for the engines that
/// clone from real audio: a local custom clip in `voice_dir` takes priority
/// over the built-in `hf://` clip. Returns a reference the caller still has to
/// resolve/download (see [`crate::audiocpp::resolve_hf_reference`]) — a plain
/// local path is returned as-is.
pub(crate) fn resolve_wav_reference_clip(id: &str, voice_dir: &str) -> Option<String> {
    if let Some((_, path)) =
        scan_custom_pocket_tts_voices(voice_dir).into_iter().find(|(cid, _)| cid == id)
    {
        return Some(path.to_string_lossy().into_owned());
    }
    builtin_wav_reference_clip(id).map(str::to_string)
}

// ── Pocket-TTS GGUF assets ─────────────────────────────────────────────────────

const POCKET_TTS_GGUF_REPO: &str = "audio-cpp/audio.cpp-gguf";
const POCKET_TTS_GGUF_FILE: &str = "PocketTTS-GGUF/english/pocket-tts-english-q8_0.gguf";
const POCKET_TTS_MODEL_FILENAME: &str = "pocket-tts-english-q8_0.gguf";

pub fn pocket_tts_model_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("models")
        .join("pocket-tts")
}

fn embedding_hf_reference(voice_id: &str) -> String {
    format!("hf://{POCKET_TTS_GGUF_REPO}/PocketTTS-GGUF/english/embeddings/{voice_id}.safetensors")
}

fn embedding_path(model_dir: &std::path::Path, voice_id: &str) -> PathBuf {
    model_dir.join("embeddings").join(format!("{voice_id}.safetensors"))
}

/// Best-effort, network-free check for whether the Pocket-TTS model and the
/// selected voice's assets are already present on disk.
pub fn is_pocket_tts_ready(voice: &str, voice_dir: &str) -> bool {
    let model_dir = pocket_tts_model_dir();
    if !model_dir.join(POCKET_TTS_MODEL_FILENAME).exists() {
        return false;
    }
    match resolve_pocket_tts_voice(voice, voice_dir) {
        Some(ResolvedVoice::BuiltIn(id)) => embedding_path(&model_dir, id).exists(),
        Some(ResolvedVoice::Custom(path)) => path.exists(),
        None => false,
    }
}

/// Downloads the Pocket-TTS GGUF model and the selected voice's embedding (or,
/// for a custom clip, does nothing further — it is already on disk). The
/// audio.cpp GGUF mirror is not gated, so `hf_token` is accepted for parity
/// with the other engines but not required.
pub async fn download_pocket_tts_assets(voice: &str, voice_dir: &str, hf_token: Option<String>) -> Result<()> {
    if audiocpp::audiocpp_binary().is_none() {
        audiocpp::download_audiocpp_binary().await.context("download audio.cpp runtime")?;
    }

    let model_dir = pocket_tts_model_dir();
    tokio::fs::create_dir_all(&model_dir)
        .await
        .with_context(|| format!("create pocket-tts model dir {}", model_dir.display()))?;

    let model_dest = model_dir.join(POCKET_TTS_MODEL_FILENAME);
    if !model_dest.exists() {
        info!("Downloading Pocket-TTS model ({POCKET_TTS_GGUF_FILE})...");
        let downloaded =
            resolve_hf_reference(&format!("hf://{POCKET_TTS_GGUF_REPO}/{POCKET_TTS_GGUF_FILE}"), hf_token.as_deref())
                .await
                .context("download pocket-tts model")?;
        tokio::fs::copy(&downloaded, &model_dest).await.context("place pocket-tts model")?;
    }

    match resolve_pocket_tts_voice(voice, voice_dir) {
        Some(ResolvedVoice::BuiltIn(id)) => {
            let dest = embedding_path(&model_dir, id);
            if !dest.exists() {
                info!("Downloading Pocket-TTS voice embedding: {id}");
                let downloaded = resolve_hf_reference(&embedding_hf_reference(id), hf_token.as_deref())
                    .await
                    .context("download pocket-tts voice embedding")?;
                if let Some(parent) = dest.parent() {
                    tokio::fs::create_dir_all(parent).await?;
                }
                tokio::fs::copy(&downloaded, &dest).await.context("place pocket-tts voice embedding")?;
            }
        }
        Some(ResolvedVoice::Custom(_)) => {}
        None => anyhow::bail!("unknown pocket-tts voice: {voice}"),
    }

    info!("pocket-tts assets ready for voice '{voice}'");
    Ok(())
}

// ── Pocket-TTS synthesis (audio.cpp subprocess) ───────────────────────────────

/// Called from `TtsEngineWorker::run` (in `engine.rs`) when `config.engine ==
/// TtsEngine::PocketTts`.
pub(crate) fn speak_pocket_tts(
    config: &voxctrl_config::TtsConfig,
    u: &crate::engine::Utterance,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    _generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    _generation: u32,
) -> Result<()> {
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let voice = u.voice.as_deref().unwrap_or(&config.pocket_tts.voice);
    let cfg = &config.pocket_tts;

    if !is_pocket_tts_ready(voice, &cfg.voice_dir) {
        anyhow::bail!("pocket-tts assets for voice '{voice}' not found. Download them from TTS settings.");
    }
    if is_prewarm {
        return Ok(());
    }

    let model_dir = pocket_tts_model_dir();
    let resolved = resolve_pocket_tts_voice(voice, &cfg.voice_dir)
        .ok_or_else(|| anyhow::anyhow!("unknown pocket-tts voice: {voice}"))?;
    let speaker = match &resolved {
        ResolvedVoice::BuiltIn(id) => SpeakerRef::Clone(&embedding_path(&model_dir, id)),
        ResolvedVoice::Custom(path) => SpeakerRef::Clone(path),
    };

    let out_wav = tempfile::Builder::new()
        .prefix("voxctrl-pocket-tts-")
        .suffix(".wav")
        .tempfile()
        .context("create temp wav for pocket-tts")?;

    synthesize(
        &SynthesizeRequest {
            family: audiocpp::FAMILY_POCKET_TTS,
            model_dir: &model_dir,
            gpu: cfg.gpu,
            text: &u.text,
            speaker: Some(speaker),
            reference_text: None,
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
    use std::fs;
    use tempfile::tempdir;

    // ── Pocket-TTS voice catalogue ──────────────────────────────────────────────

    #[test]
    fn test_pocket_tts_voices_not_empty() {
        assert!(!POCKET_TTS_VOICES.is_empty());
    }

    #[test]
    fn test_pocket_tts_voices_have_required_fields() {
        for v in POCKET_TTS_VOICES {
            assert!(!v.id.is_empty());
            assert!(!v.label.is_empty());
        }
    }

    #[test]
    fn test_pocket_tts_voices_ids_unique() {
        let mut seen = std::collections::HashSet::new();
        for v in POCKET_TTS_VOICES {
            assert!(seen.insert(v.id), "duplicate voice id: {}", v.id);
        }
    }

    #[test]
    fn test_pocket_tts_voice_lookup_known() {
        assert!(pocket_tts_voice("alba").is_some());
        assert!(pocket_tts_voice("michael").is_some());
    }

    #[test]
    fn test_pocket_tts_voice_lookup_unknown_returns_none() {
        assert!(pocket_tts_voice("not-a-real-voice").is_none());
    }

    // ── is_pocket_tts_ready ──────────────────────────────────────────────────

    #[test]
    fn test_is_pocket_tts_ready_false_for_unknown_voice() {
        assert!(!is_pocket_tts_ready("not-a-real-voice", ""));
    }

    #[test]
    fn test_is_pocket_tts_ready_false_without_model() {
        // Even a built-in voice isn't "ready" until the shared model file exists.
        assert!(!is_pocket_tts_ready("alba", ""));
    }

    // ── custom voice directory ───────────────────────────────────────────────

    #[test]
    fn test_scan_custom_pocket_tts_voices_finds_wav_files() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("myvoice.wav"), b"fake audio").unwrap();
        fs::write(dir.path().join("notes.txt"), b"ignore me").unwrap();
        let found = scan_custom_pocket_tts_voices(dir.path().to_str().unwrap());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "myvoice");
    }

    #[test]
    fn test_pocket_tts_voice_catalogue_merges_custom_voices() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("myvoice.wav"), b"fake audio").unwrap();
        let catalogue = pocket_tts_voice_catalogue(dir.path().to_str().unwrap());
        assert!(catalogue.iter().any(|v| v.id == "myvoice"));
        assert!(catalogue.iter().any(|v| v.id == "alba"));
    }

    #[test]
    fn test_pocket_tts_voice_catalogue_custom_overrides_builtin_label() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("alba.wav"), b"fake audio").unwrap();
        let catalogue = pocket_tts_voice_catalogue(dir.path().to_str().unwrap());
        let alba = catalogue.iter().find(|v| v.id == "alba").unwrap();
        assert!(alba.label.contains("Custom"));
    }

    #[test]
    fn test_resolve_pocket_tts_voice_prefers_custom() {
        let dir = tempdir().unwrap();
        let clip = dir.path().join("alba.wav");
        fs::write(&clip, b"fake audio").unwrap();
        match resolve_pocket_tts_voice("alba", dir.path().to_str().unwrap()) {
            Some(ResolvedVoice::Custom(path)) => assert_eq!(path, clip),
            _ => panic!("expected custom voice to win"),
        }
    }

    #[test]
    fn test_resolve_pocket_tts_voice_falls_back_to_builtin() {
        let dir = tempdir().unwrap();
        match resolve_pocket_tts_voice("alba", dir.path().to_str().unwrap()) {
            Some(ResolvedVoice::BuiltIn(id)) => assert_eq!(id, "alba"),
            _ => panic!("expected built-in voice"),
        }
    }

    #[test]
    fn test_resolve_pocket_tts_voice_unknown_returns_none() {
        let dir = tempdir().unwrap();
        assert!(resolve_pocket_tts_voice("not-a-real-voice", dir.path().to_str().unwrap()).is_none());
    }
}
