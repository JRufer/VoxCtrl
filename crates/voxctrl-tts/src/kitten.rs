//! Kitten TTS 2 neural text-to-speech engine support, via the shared
//! audio.cpp runtime (see `audiocpp.rs`) and its `kitten_tts2` family.
//!
//! Model: <https://huggingface.co/KittenML/kitten-tts-2> — a 1.7B-parameter
//! multilingual speech language model. VoxCtrl downloads audio.cpp's GGUF
//! conversion from <https://huggingface.co/audio-cpp/audio.cpp-gguf>.
//! The weights are under the Stellon Labs Community License — see
//! [`KITTEN_TTS_2_LICENSE_NOTE`] and the Settings/setup-wizard warnings
//! before download.
//!
//! Two ways to pick a voice: a preset built into the GGUF (`voice`, e.g.
//! "Bruno"), or cloning from a reference clip in the shared voice folder.
//! Kitten requires the clip's exact transcript to clone, exactly like
//! Breeze-TTS-2, so a clone voice needs `<id>.txt` next to `<id>.wav`.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::{KittenTts2Config, TtsConfig};

use crate::audiocpp::{
    self, resolve_hf_reference_blocking, resolve_model_dir, AudioCppRuntime, AudioCppSession, SpeakRequest, SpeakerRef,
};
use crate::pocket::resolve_wav_reference_clip;
use crate::utterance::{PlaybackCallback, Utterance};

pub const KITTEN_TTS_2_SAMPLE_RATE: u32 = 24_000;
const KITTEN_TTS_2_GGUF_REPO: &str = "audio-cpp/audio.cpp-gguf";
/// The package audio.cpp documents as its default and recommended one
/// (Q8_0 body, BF16 embedding, 3.3 GB).
const KITTEN_TTS_2_GGUF_FILE: &str = "KittenTTS2-GGUF/kitten-tts2-20261021-q8_0.gguf";
const KITTEN_TTS_2_MODEL_FILENAME: &str = "kitten-tts2-20261021-q8_0.gguf";
pub const KITTEN_TTS_2_DOWNLOAD_BYTES: u64 = 3_340_000_000;

/// Surfaced by Settings and the setup wizard before a Kitten TTS 2 download,
/// and on the About tab. "Powered by Stellon Labs" is the attribution the
/// licence asks an app that ships with the model to display.
pub const KITTEN_TTS_2_LICENSE_NOTE: &str = "Kitten TTS 2 model weights are released under the Stellon Labs Community \
     License. Personal, research and non-commercial use are free. Commercial use \
     requires registering with Stellon Labs and stops being free above USD $1M in \
     annual revenue or funding. Audio it produces may not be used to train other \
     models. Powered by Stellon Labs.";

/// The preset voices in the default package: the 38 named English voices
/// and the nine language voices, which is also how a non-English language is
/// chosen (there is no separate language switch). Newer conversions of the
/// model carry more character voices; Settings lets you type any name, and
/// audio.cpp reports an unknown one clearly.
pub const KITTEN_TTS_2_VOICES: &[&str] = &[
    "Bella",
    "Jasper",
    "Luna",
    "Bruno",
    "Rosie",
    "Hugo",
    "Kiki",
    "Leo",
    "Matthew",
    "Elliot",
    "Willow",
    "Dolores",
    "Victor",
    "Dante",
    "Alfred",
    "Saoirse",
    "Claire",
    "Raven",
    "Marcus",
    "Herbert",
    "Diana",
    "Laurence",
    "Maeve",
    "Walter",
    "Edith",
    "Miles",
    "Grace",
    "Reginald",
    "Iris",
    "Frank",
    "Serena",
    "Julian",
    "Eleanor",
    "Otis",
    "Vincent",
    "Martha",
    "Sable",
    "Victoria",
    "Arabic",
    "Chinese",
    "French",
    "German",
    "Hindi",
    "Italian",
    "Portuguese",
    "Russian",
    "Spanish",
];

/// Language-named presets: text for these is not English, so the English
/// number speller must not touch it.
const NON_ENGLISH_PRESETS: &[&str] =
    &["Arabic", "Chinese", "French", "German", "Hindi", "Italian", "Portuguese", "Russian", "Spanish"];

const DEFAULT_VOICE: &str = "Bruno";

pub fn kitten_tts_2_model_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("voxctrl").join("models").join("kitten-tts-2")
}

pub fn resolve_kitten_tts_2_dir(model_dir: &str) -> PathBuf {
    resolve_model_dir(model_dir, kitten_tts_2_model_dir)
}

fn model_file(model_dir: &str) -> PathBuf {
    resolve_kitten_tts_2_dir(model_dir).join(KITTEN_TTS_2_MODEL_FILENAME)
}

/// Ready means the model is on disk *and* a runtime that can run it is
/// installed. An audio.cpp install from before v0.9.4 lacks the `kitten_tts2`
/// family, and a missing one can't run anything, so a model copied in by hand
/// in either situation is not ready — the Download button then installs or
/// upgrades the runtime without re-fetching the model. (A system-wide
/// `audiocpp_server` is trusted; VoxCtrl can't know what it supports.)
pub fn is_kitten_tts_2_ready(model_dir: &str) -> bool {
    model_file(model_dir).exists() && runtime_can_run_kitten(audiocpp::audiocpp_runtime())
}

fn runtime_can_run_kitten(runtime: AudioCppRuntime) -> bool {
    matches!(runtime, AudioCppRuntime::Current | AudioCppRuntime::System | AudioCppRuntime::Custom)
}

/// Downloads the Kitten TTS 2 GGUF into `model_dir`, installing or upgrading
/// the audio.cpp runtime first. The GGUF mirror is not gated, so `hf_token`
/// is accepted for parity with the other engines but not required.
pub async fn download_kitten_tts_2_assets(model_dir: &str, hf_token: Option<String>) -> Result<()> {
    audiocpp::ensure_audiocpp_runtime().await?;

    let dir = resolve_kitten_tts_2_dir(model_dir);
    tokio::fs::create_dir_all(&dir)
        .await
        .with_context(|| format!("create kitten-tts-2 model dir {}", dir.display()))?;

    let dest = dir.join(KITTEN_TTS_2_MODEL_FILENAME);
    if !dest.exists() {
        info!("Downloading Kitten TTS 2 model ({KITTEN_TTS_2_GGUF_FILE}, ~3.3 GB)...");
        audiocpp::download_hf_file(
            &format!("hf://{KITTEN_TTS_2_GGUF_REPO}/{KITTEN_TTS_2_GGUF_FILE}"),
            &dest,
            hf_token.as_deref(),
        )
        .await
        .context("download kitten-tts-2 model")?;
    }

    info!("Kitten TTS 2 assets ready in {}", dir.display());
    Ok(())
}

fn read_voice_transcript_file(wav_path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(wav_path.with_extension("txt")).ok()?;
    let trimmed = content.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn is_clone_mode(cfg: &KittenTts2Config) -> bool {
    cfg.voice_mode == "clone"
}

fn preset_voice(cfg: &KittenTts2Config) -> &str {
    let v = cfg.voice.trim();
    if v.is_empty() {
        DEFAULT_VOICE
    } else {
        v
    }
}

/// The reference clip and its (mandatory) transcript, or `None` in preset mode.
fn resolve_clone_reference(cfg: &KittenTts2Config, hf_token: Option<&str>) -> Result<Option<(PathBuf, String)>> {
    if !is_clone_mode(cfg) {
        return Ok(None);
    }
    let voice_id = cfg.cloned_voice.trim();
    if voice_id.is_empty() {
        anyhow::bail!("Kitten TTS 2 is set to voice cloning, but no voice clip is selected.");
    }
    let reference = resolve_wav_reference_clip(voice_id, &cfg.voice_dir)
        .ok_or_else(|| anyhow::anyhow!("Kitten TTS 2 voice clip '{voice_id}' was not found in the voice folder."))?;
    let path =
        resolve_hf_reference_blocking(&reference, hf_token).context("resolve Kitten TTS 2 reference voice clip")?;
    // audio.cpp refuses a kitten_tts2 clone without `reference_text`.
    let transcript = read_voice_transcript_file(&path).ok_or_else(|| {
        anyhow::anyhow!(
            "Kitten TTS 2 voice cloning needs a transcript: add a {}.txt file next to the \
             reference clip containing exactly what is spoken in it.",
            path.file_stem().and_then(|s| s.to_str()).unwrap_or("<voice>")
        )
    })?;
    Ok(Some((path, transcript)))
}

/// The `options` audio.cpp accepts for this family that VoxCtrl sets.
fn request_options(cfg: &KittenTts2Config) -> [(&'static str, &'static str); 1] {
    let preset = if cfg.preset == "expressive" { "expressive" } else { "stable" };
    [("preset", preset)]
}

/// Text as Kitten should receive it. audio.cpp's port leaves out the
/// upstream English normalizer, so digits, money and the like are spelled
/// out here — unless the voice is one of the language presets, whose text is
/// not English. Expression tags (`[excited]`, `<laugh>`) pass through as-is.
fn prepare_text(cfg: &KittenTts2Config, text: &str) -> String {
    let english = is_clone_mode(cfg) || !NON_ENGLISH_PRESETS.contains(&preset_voice(cfg));
    if english {
        voxctrl_text::spell_out_numbers_en(text)
    } else {
        text.to_string()
    }
}

fn ensure_session<'a>(config: &TtsConfig, session: &'a mut Option<AudioCppSession>) -> Result<&'a AudioCppSession> {
    let cfg = &config.kitten_tts_2;
    if !model_file(&cfg.model_dir).exists() {
        anyhow::bail!("Kitten TTS 2 model not found. Download it from TTS settings.");
    }
    // Before the check below, so an out-of-date runtime is replaced rather
    // than reported, whenever it can be.
    audiocpp::upgrade_outdated_runtime_blocking();
    if audiocpp::audiocpp_runtime() == AudioCppRuntime::Outdated {
        anyhow::bail!(
            "Kitten TTS 2 needs a newer audio.cpp runtime ({}). Click Download in TTS \
             settings to update it.",
            audiocpp::AUDIOCPP_RELEASE_VERSION
        );
    }
    let model_dir = resolve_kitten_tts_2_dir(&cfg.model_dir);
    AudioCppSession::ensure(session, audiocpp::FAMILY_KITTEN_TTS2, &model_dir, cfg.gpu)?;
    Ok(session.as_ref().expect("ensure() fills the session"))
}

fn synthesize(config: &TtsConfig, session: &AudioCppSession, text: &str) -> Result<Vec<u8>> {
    let cfg = &config.kitten_tts_2;
    let clone_ref = resolve_clone_reference(cfg, config.hf_token.as_deref())?;
    let speaker = match &clone_ref {
        Some((path, _)) => SpeakerRef::Clone(path),
        None => SpeakerRef::VoiceId(preset_voice(cfg)),
    };
    let options = request_options(cfg);
    session.speak(&SpeakRequest {
        text,
        speaker: Some(speaker),
        reference_text: clone_ref.as_ref().map(|(_, t)| t.as_str()),
        options: &options,
    })
}

/// Ensures a resident audio.cpp session is loaded and warm for Kitten TTS 2.
/// Called from `TtsCommand::Preload` — the server loads lazily, so only a
/// real (short) request actually pulls the weights into memory.
pub(crate) fn ensure_kitten_tts_2_loaded(config: &TtsConfig, session: &mut Option<AudioCppSession>) -> Result<()> {
    let s = ensure_session(config, session)?;
    synthesize(config, s, "Hi.")?;
    Ok(())
}

/// Called from `TtsEngineWorker::run` when `config.engine ==
/// TtsEngine::KittenTts2`. The session persists across calls (keeping the
/// model loaded) for the worker's lifetime, or until idle-unload drops it.
pub(crate) fn synth_kitten_tts_2(
    config: &TtsConfig,
    u: &Utterance,
    session: &mut Option<AudioCppSession>,
) -> Result<Vec<u8>> {
    let cfg = &config.kitten_tts_2;
    let s = ensure_session(config, session)?;

    info!(
        "Synthesizing text with Kitten TTS 2 (voice={}, preset={}, gpu={})...",
        if is_clone_mode(cfg) { cfg.cloned_voice.trim() } else { preset_voice(cfg) },
        request_options(cfg)[0].1,
        cfg.gpu
    );
    // audio.cpp rejects empty input for this family.
    let text = prepare_text(cfg, &u.text);
    let text = if text.trim().is_empty() { "Hi.".to_string() } else { text };
    let audio = synthesize(config, s, &text)?;

    Ok(audio)
}

/// [`synth_kitten_tts_2`], then played on `sink`. A pre-warm utterance is
/// synthesized, to load the model, and not played.
#[cfg(feature = "playback")]
pub(crate) fn speak_kitten_tts_2(
    config: &TtsConfig,
    u: &Utterance,
    session: &mut Option<AudioCppSession>,
    on_playback_start: &Option<PlaybackCallback>,
    sink: &rodio::Sink,
    _generation_counter: &Arc<std::sync::atomic::AtomicU32>,
    _generation: u32,
) -> Result<()> {
    let is_prewarm = u.source_label.as_deref() == Some("prewarm");
    let audio = synth_kitten_tts_2(config, u, session)?;
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
    fn test_kitten_tts_2_model_dir() {
        assert!(kitten_tts_2_model_dir().ends_with("kitten-tts-2"));
    }

    #[test]
    fn test_resolve_kitten_tts_2_dir() {
        assert_eq!(resolve_kitten_tts_2_dir(""), kitten_tts_2_model_dir());
        assert_eq!(resolve_kitten_tts_2_dir("/tmp/kitten"), PathBuf::from("/tmp/kitten"));
    }

    #[test]
    fn test_is_kitten_tts_2_ready_false_when_empty() {
        let dir = tempdir().unwrap();
        assert!(!is_kitten_tts_2_ready(dir.path().to_str().unwrap()));
    }

    #[test]
    fn test_only_a_current_system_or_custom_runtime_can_run_kitten() {
        assert!(runtime_can_run_kitten(AudioCppRuntime::Current));
        assert!(runtime_can_run_kitten(AudioCppRuntime::System));
        assert!(runtime_can_run_kitten(AudioCppRuntime::Custom));
        assert!(!runtime_can_run_kitten(AudioCppRuntime::Outdated));
        assert!(!runtime_can_run_kitten(AudioCppRuntime::Missing));
    }

    #[test]
    fn test_default_voice_is_a_listed_preset() {
        assert!(KITTEN_TTS_2_VOICES.contains(&DEFAULT_VOICE));
        assert_eq!(preset_voice(&KittenTts2Config::default()), DEFAULT_VOICE);
        let blank = KittenTts2Config { voice: "  ".into(), ..Default::default() };
        assert_eq!(preset_voice(&blank), DEFAULT_VOICE);
    }

    #[test]
    fn test_preset_option_only_sends_known_values() {
        let mut cfg = KittenTts2Config::default();
        assert_eq!(request_options(&cfg), [("preset", "stable")]);
        cfg.preset = "expressive".into();
        assert_eq!(request_options(&cfg), [("preset", "expressive")]);
        cfg.preset = "loud".into();
        assert_eq!(request_options(&cfg), [("preset", "stable")]);
    }

    #[test]
    fn test_numbers_spelled_out_for_english_voices_only() {
        let mut cfg = KittenTts2Config::default();
        assert_eq!(prepare_text(&cfg, "I have 3 cats."), "I have three cats.");
        cfg.voice = "German".into();
        assert_eq!(prepare_text(&cfg, "Ich habe 3 Katzen."), "Ich habe 3 Katzen.");
    }

    #[test]
    fn test_clone_mode_needs_a_selected_clip() {
        let cfg = KittenTts2Config { voice_mode: "clone".into(), ..Default::default() };
        let err = resolve_clone_reference(&cfg, None).unwrap_err().to_string();
        assert!(err.contains("no voice clip"), "{err}");
    }

    #[test]
    fn test_clone_mode_needs_a_transcript() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("me.wav"), b"RIFF").unwrap();
        let cfg = KittenTts2Config {
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
        assert!(resolve_clone_reference(&KittenTts2Config::default(), None).unwrap().is_none());
    }
}
