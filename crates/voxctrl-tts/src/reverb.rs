//! Reverb-TTS neural text-to-speech engine support, via the shared audio.cpp
//! runtime (see `audiocpp.rs`) and its `reverb` family.
//!
//! Model: <https://huggingface.co/software-mansion/Reverb-TTS> by Software
//! Mansion — a 297M-parameter zero-shot voice-cloning model (a pruned,
//! retrained OmniVoice) for English, German, French, Spanish, Italian, Polish
//! and Portuguese. VoxCtrl downloads the audio.cpp GGUF conversion from
//! <https://huggingface.co/JRufer/Reverb-TTS-GGUF>. The weights are CC BY-NC
//! 4.0 — see [`REVERB_LICENSE_NOTE`] and the Settings/setup-wizard warnings
//! before download.
//!
//! The `reverb` family is not in an audio.cpp release yet (see
//! [`audiocpp::AUDIOCPP_RELEASE_HAS_REVERB`]), so the engine needs an
//! `audiocpp_server` that has it: `tts.audiocpp_server`, or one on PATH.
//!
//! Two ways to pick a voice: one of the 16 presets built into the GGUF, or
//! cloning from a reference clip in the shared voice folder. Like Kitten TTS 2,
//! a clone needs the clip's exact transcript in `<id>.txt` next to `<id>.wav`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::{ReverbConfig, TtsConfig};

use crate::audiocpp::{
    self, resolve_hf_reference_blocking, resolve_model_dir, AudioCppRuntime, AudioCppSession, SpeakRequest, SpeakerRef,
};
use crate::pocket::resolve_wav_reference_clip;
use crate::utterance::{PlaybackCallback, Utterance};

pub const REVERB_SAMPLE_RATE: u32 = 24_000;
const REVERB_GGUF_REPO: &str = "JRufer/Reverb-TTS-GGUF";
/// Q8_0 generator (about 340 MB) plus the Higgs Audio v2 codec, one file.
const REVERB_MODEL_FILENAME: &str = "reverb-tts-q8_0.gguf";
pub const REVERB_DOWNLOAD_BYTES: u64 = 875_481_440;

/// Surfaced by Settings and the setup wizard before a Reverb-TTS download, and
/// on the About tab.
pub const REVERB_LICENSE_NOTE: &str = "Reverb-TTS model weights by Software Mansion are released under CC BY-NC \
     4.0: non-commercial use only, with attribution. The bundled Higgs Audio v2 \
     codec is under the Boson Higgs Audio 2 Community License. Clone only voices \
     you have the right to use, and tell listeners when speech is synthetic.";

/// The preset voices in the package. The prefix is the voice's own language,
/// which it speaks most naturally; any voice can speak any of the seven.
pub const REVERB_VOICES: &[&str] = &[
    "en_female_narrator",
    "en_female_whisper",
    "en_male_angry",
    "en_male_narrator_1",
    "en_male_narrator_2",
    "de_female_standard",
    "de_male_standard",
    "es_male_standard_1",
    "es_male_standard_2",
    "fr_female_standard",
    "fr_male_standard",
    "it_male_expressive",
    "it_male_standard",
    "pl_male_standard",
    "pt_female_standard",
    "pt_male_standard",
];

/// The languages Reverb speaks, as audio.cpp's `language` option takes them.
pub const REVERB_LANGUAGES: &[&str] = &["en", "de", "fr", "es", "it", "pl", "pt"];

const DEFAULT_VOICE: &str = "en_female_narrator";

pub fn reverb_model_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("voxctrl").join("models").join("reverb-tts")
}

pub fn resolve_reverb_dir(model_dir: &str) -> PathBuf {
    resolve_model_dir(model_dir, reverb_model_dir)
}

fn model_file(model_dir: &str) -> PathBuf {
    resolve_reverb_dir(model_dir).join(REVERB_MODEL_FILENAME)
}

/// Whether the runtime the audio.cpp engines would use right now can run the
/// `reverb` family: one the user supplied (`tts.audiocpp_server`, or PATH with
/// no managed install), or VoxCtrl's own once its pinned release has it.
pub fn reverb_runtime_supported() -> bool {
    runtime_can_run_reverb(audiocpp::audiocpp_runtime())
}

fn runtime_can_run_reverb(runtime: AudioCppRuntime) -> bool {
    match runtime {
        AudioCppRuntime::Custom | AudioCppRuntime::System | AudioCppRuntime::Bundled => true,
        AudioCppRuntime::Current => audiocpp::AUDIOCPP_RELEASE_HAS_REVERB,
        AudioCppRuntime::Outdated | AudioCppRuntime::Missing => false,
    }
}

/// Ready means the model is on disk *and* a runtime that can run it is there.
/// Why Reverb-TTS cannot run on the current audio.cpp runtime.
pub(crate) const REVERB_RUNTIME_MISSING: &str =
    "Reverb-TTS needs an audio.cpp server with the `reverb` model family, which no audio.cpp release \
     has yet. Set \"audio.cpp server\" in TTS settings (tts.audiocpp_server, or \
     VOXCTRL_SERVER_AUDIOCPP_SERVER for the server) to an audiocpp_server built from source, or build \
     the server's Docker image with VOXCTRL_BUILD_AUDIOCPP_REF=reverb-tts (see docs/tts.md).";

pub fn is_reverb_ready(model_dir: &str) -> bool {
    is_reverb_downloaded(model_dir) && reverb_runtime_supported()
}

/// Whether the Reverb-TTS GGUF is on disk, whatever the runtime. Settings
/// shows this and the runtime check separately, so a finished download reads
/// as finished even before "audio.cpp server" is set.
pub fn is_reverb_downloaded(model_dir: &str) -> bool {
    model_file(model_dir).exists()
}

/// Downloads the Reverb-TTS GGUF into `model_dir`, installing or upgrading the
/// managed audio.cpp runtime first (left alone when `tts.audiocpp_server` is
/// set). The repository is not gated; `hf_token` is accepted for parity.
pub async fn download_reverb_assets(model_dir: &str, hf_token: Option<String>) -> Result<()> {
    audiocpp::ensure_audiocpp_runtime().await?;

    let dir = resolve_reverb_dir(model_dir);
    tokio::fs::create_dir_all(&dir).await.with_context(|| format!("create reverb-tts model dir {}", dir.display()))?;

    let dest = dir.join(REVERB_MODEL_FILENAME);
    if !dest.exists() {
        info!("Downloading Reverb-TTS model ({REVERB_GGUF_REPO}/{REVERB_MODEL_FILENAME}, ~875 MB)...");
        audiocpp::download_hf_file(
            &format!("hf://{REVERB_GGUF_REPO}/{REVERB_MODEL_FILENAME}"),
            &dest,
            hf_token.as_deref(),
        )
        .await
        .context("download reverb-tts model")?;
    }

    info!("Reverb-TTS assets ready in {}", dir.display());
    Ok(())
}

fn read_voice_transcript_file(wav_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(wav_path.with_extension("txt")).ok()?;
    let trimmed = content.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn is_clone_mode(cfg: &ReverbConfig) -> bool {
    cfg.voice_mode == "clone"
}

fn preset_voice(cfg: &ReverbConfig) -> &str {
    let v = cfg.voice.trim();
    if v.is_empty() {
        DEFAULT_VOICE
    } else {
        v
    }
}

/// The language of the text: the configured one when it is one Reverb speaks,
/// else the preset's own (its `xx_` prefix), else English (a clone).
fn language(cfg: &ReverbConfig) -> &str {
    let configured = cfg.language.trim();
    if REVERB_LANGUAGES.contains(&configured) {
        return configured;
    }
    if !is_clone_mode(cfg) {
        if let Some(prefix) = preset_voice(cfg).get(..2) {
            if REVERB_LANGUAGES.contains(&prefix) {
                return prefix;
            }
        }
    }
    "en"
}

/// The reference clip and its (mandatory) transcript, or `None` in preset mode.
fn resolve_clone_reference(cfg: &ReverbConfig, hf_token: Option<&str>) -> Result<Option<(PathBuf, String)>> {
    if !is_clone_mode(cfg) {
        return Ok(None);
    }
    let voice_id = cfg.cloned_voice.trim();
    if voice_id.is_empty() {
        anyhow::bail!("Reverb-TTS is set to voice cloning, but no voice clip is selected.");
    }
    let reference = resolve_wav_reference_clip(voice_id, &cfg.voice_dir)
        .ok_or_else(|| anyhow::anyhow!("Reverb-TTS voice clip '{voice_id}' was not found in the voice folder."))?;
    let path =
        resolve_hf_reference_blocking(&reference, hf_token).context("resolve Reverb-TTS reference voice clip")?;
    // audio.cpp refuses a reverb clone without `reference_text`.
    let transcript = read_voice_transcript_file(&path).ok_or_else(|| {
        anyhow::anyhow!(
            "Reverb-TTS voice cloning needs a transcript: add a {}.txt file next to the \
             reference clip containing exactly what is spoken in it.",
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("<voice>")
        )
    })?;
    Ok(Some((path, transcript)))
}

/// Text as Reverb should receive it. Upstream verbalizes text (numbers, money,
/// symbols) before the model sees it; audio.cpp's port does not, so for English
/// digits and the like are spelled out here. Other languages pass through.
fn prepare_text(language: &str, text: &str) -> String {
    if language == "en" {
        voxctrl_text::spell_out_numbers_en(text)
    } else {
        text.to_string()
    }
}

fn ensure_session<'a>(config: &TtsConfig, session: &'a mut Option<AudioCppSession>) -> Result<&'a AudioCppSession> {
    let cfg = &config.reverb;
    if !model_file(&cfg.model_dir).exists() {
        anyhow::bail!("Reverb-TTS model not found. Download it from TTS settings.");
    }
    audiocpp::upgrade_outdated_runtime_blocking();
    if !reverb_runtime_supported() {
        anyhow::bail!(REVERB_RUNTIME_MISSING);
    }
    // The GGUF itself, not its folder: a folder holding both the Q8_0 and the
    // F16 conversion is ambiguous to audio.cpp, which then refuses to start.
    AudioCppSession::ensure(session, audiocpp::FAMILY_REVERB, &model_file(&cfg.model_dir), cfg.gpu)?;
    Ok(session.as_ref().expect("ensure() fills the session"))
}

fn synthesize(config: &TtsConfig, session: &AudioCppSession, text: &str) -> Result<Vec<u8>> {
    let cfg = &config.reverb;
    let language = language(cfg);
    let clone_ref = resolve_clone_reference(cfg, config.hf_token.as_deref())?;
    let speaker = match &clone_ref {
        Some((path, _)) => SpeakerRef::Clone(path),
        None => SpeakerRef::VoiceId(preset_voice(cfg)),
    };
    let reference_text = clone_ref.as_ref().map(|(_, t)| prepare_text(language, t));
    let options = [("language", language)];
    session.speak(&SpeakRequest {
        text,
        speaker: Some(speaker),
        reference_text: reference_text.as_deref(),
        options: &options,
    })
}

/// Ensures a resident audio.cpp session is loaded and warm for Reverb-TTS.
/// Called from `TtsCommand::Preload` — the server loads lazily, so only a real
/// (short) request actually pulls the weights into memory.
pub(crate) fn ensure_reverb_loaded(config: &TtsConfig, session: &mut Option<AudioCppSession>) -> Result<()> {
    let s = ensure_session(config, session)?;
    synthesize(config, s, "Hi.")?;
    Ok(())
}

/// Called from `TtsEngineWorker::run` when `config.engine == TtsEngine::Reverb`.
/// The session persists across calls (keeping the model loaded) for the
/// worker's lifetime, or until idle-unload drops it.
pub(crate) fn synth_reverb(
    config: &TtsConfig,
    u: &Utterance,
    session: &mut Option<AudioCppSession>,
) -> Result<Vec<u8>> {
    let cfg = &config.reverb;
    let s = ensure_session(config, session)?;

    info!(
        "Synthesizing text with Reverb-TTS (voice={}, language={}, gpu={})...",
        if is_clone_mode(cfg) { cfg.cloned_voice.trim() } else { preset_voice(cfg) },
        language(cfg),
        cfg.gpu
    );
    // audio.cpp rejects empty input for this family.
    let text = prepare_text(language(cfg), &u.text);
    let text = if text.trim().is_empty() { "Hi.".to_string() } else { text };
    synthesize(config, s, &text)
}

/// [`synth_reverb`], then played on `sink`. A pre-warm utterance is
/// synthesized, to load the model, and not played.
#[cfg(feature = "playback")]
pub(crate) fn speak_reverb(
    config: &TtsConfig,
    u: &Utterance,
    session: &mut Option<AudioCppSession>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    _generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    _generation: u32,
) -> Result<()> {
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let audio = synth_reverb(config, u, session)?;
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
    fn test_reverb_model_dir() {
        assert!(reverb_model_dir().ends_with("reverb-tts"));
        assert_eq!(resolve_reverb_dir(""), reverb_model_dir());
        assert_eq!(resolve_reverb_dir("/tmp/reverb"), PathBuf::from("/tmp/reverb"));
    }

    #[test]
    fn test_is_reverb_ready_false_when_empty() {
        let dir = tempdir().unwrap();
        assert!(!is_reverb_ready(dir.path().to_str().unwrap()));
        assert!(!is_reverb_downloaded(dir.path().to_str().unwrap()));
        std::fs::write(dir.path().join(REVERB_MODEL_FILENAME), b"GGUF").unwrap();
        assert!(is_reverb_downloaded(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_runtime_must_have_the_reverb_family() {
        assert!(runtime_can_run_reverb(AudioCppRuntime::Custom));
        assert!(runtime_can_run_reverb(AudioCppRuntime::System));
        assert!(runtime_can_run_reverb(AudioCppRuntime::Bundled));
        assert_eq!(runtime_can_run_reverb(AudioCppRuntime::Current), audiocpp::AUDIOCPP_RELEASE_HAS_REVERB);
        assert!(!runtime_can_run_reverb(AudioCppRuntime::Outdated));
        assert!(!runtime_can_run_reverb(AudioCppRuntime::Missing));
    }

    #[test]
    fn test_default_voice_is_a_listed_preset() {
        assert!(REVERB_VOICES.contains(&DEFAULT_VOICE));
        assert_eq!(preset_voice(&ReverbConfig::default()), DEFAULT_VOICE);
        let blank = ReverbConfig { voice: "  ".into(), ..Default::default() };
        assert_eq!(preset_voice(&blank), DEFAULT_VOICE);
        for voice in REVERB_VOICES {
            assert!(REVERB_LANGUAGES.contains(&&voice[..2]), "{voice}");
        }
    }

    #[test]
    fn test_language_follows_setting_then_preset_then_english() {
        let mut cfg = ReverbConfig::default();
        assert_eq!(language(&cfg), "en");
        cfg.voice = "fr_female_standard".into();
        assert_eq!(language(&cfg), "fr");
        cfg.language = "de".into();
        assert_eq!(language(&cfg), "de");
        cfg.language = "nl".into(); // not one Reverb speaks: ignored
        assert_eq!(language(&cfg), "fr");
        let clone =
            ReverbConfig { voice_mode: "clone".into(), voice: "fr_female_standard".into(), ..Default::default() };
        assert_eq!(language(&clone), "en");
    }

    #[test]
    fn test_numbers_spelled_out_for_english_only() {
        assert_eq!(prepare_text("en", "I have 3 cats."), "I have three cats.");
        assert_eq!(prepare_text("de", "Ich habe 3 Katzen."), "Ich habe 3 Katzen.");
    }

    #[test]
    fn test_clone_mode_needs_a_selected_clip_and_transcript() {
        let cfg = ReverbConfig { voice_mode: "clone".into(), ..Default::default() };
        let err = resolve_clone_reference(&cfg, None).unwrap_err().to_string();
        assert!(err.contains("no voice clip"), "{err}");

        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("me.wav"), b"RIFF").unwrap();
        let cfg = ReverbConfig {
            voice_mode: "clone".into(),
            cloned_voice: "me".into(),
            voice_dir: dir.path().to_string_lossy().into_owned(),
            ..Default::default()
        };
        let err = resolve_clone_reference(&cfg, None).unwrap_err().to_string();
        assert!(err.contains("transcript"), "{err}");
        std::fs::write(dir.path().join("me.txt"), "  Hello, this is me.\n").unwrap();
        let (path, transcript) = resolve_clone_reference(&cfg, None).unwrap().unwrap();
        assert!(path.ends_with("me.wav"));
        assert_eq!(transcript, "Hello, this is me.");
    }

    #[test]
    fn test_preset_mode_has_no_clone_reference() {
        assert!(resolve_clone_reference(&ReverbConfig::default(), None).unwrap().is_none());
    }
}
