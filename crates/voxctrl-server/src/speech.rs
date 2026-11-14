//! Text-to-speech on the server: the engine and voice set under `[tts]` in
//! `server.toml`, for clients whose TTS engine is "VoxCtrl server".
//!
//! The server synthesizes and returns a WAV file; the client plays it. A
//! client's own speed setting and stop key still apply, since playback is
//! theirs.

use std::sync::Mutex;

use anyhow::{Context, Result};
use tracing::info;
use voxctrl_config::{TtsConfig, TtsEngine};
use voxctrl_tts::synth::{self, Synthesizer};

/// Something that can turn text into a WAV file. A trait so the HTTP layer
/// can be tested without an engine installed.
pub trait Speaker: Send + Sync {
    /// Blocking, and can take seconds: call it off the async runtime.
    fn speak(&self, text: &str) -> Result<Vec<u8>>;
    /// The engine's name as config files spell it, e.g. `piper`.
    fn engine(&self) -> String;
    /// The voice it speaks with, where the engine has named voices.
    fn voice(&self) -> Option<String>;
}

/// One of the app's own TTS engines, with its model kept loaded.
pub struct EngineSpeaker {
    config: TtsConfig,
    // One request at a time: the engines hold one model and one helper process.
    synthesizer: Mutex<Synthesizer>,
}

impl EngineSpeaker {
    /// Download what the engine and voice need, and load the model, so the
    /// first request pays for neither.
    pub async fn prepare(config: TtsConfig) -> Result<Self> {
        if !synth::supports(&config.engine) {
            anyhow::bail!(
                "tts.engine = \"{}\" cannot run on the server. Use piper, espeak, pocket_tts, breeze_tts_2, \
                 vox_cpm_2 or kitten_tts_2.",
                synth::engine_id(&config.engine)
            );
        }
        info!("Preparing text-to-speech ({})", synth::engine_id(&config.engine));
        synth::ensure_assets(&config).await.context(
            "Could not get the text-to-speech engine ready. Check this machine's internet access, \
             or turn text-to-speech off (tts.enabled = false).",
        )?;
        let (synthesizer, config) = tokio::task::spawn_blocking(move || {
            let mut synthesizer = Synthesizer::new();
            synthesizer.load(&config).map(|()| (synthesizer, config))
        })
        .await
        .context("Loading the text-to-speech model stopped unexpectedly")?
        .context("Could not load the text-to-speech model")?;
        Ok(Self { config, synthesizer: Mutex::new(synthesizer) })
    }
}

/// The voice the configured engine speaks with, for engines that name one.
pub fn configured_voice(config: &TtsConfig) -> Option<String> {
    let named = |voice: &str| Some(voice.to_string()).filter(|v| !v.trim().is_empty());
    match config.engine {
        TtsEngine::Piper => named(&config.voice),
        TtsEngine::PocketTts => named(&config.pocket_tts.voice),
        TtsEngine::KittenTts2 => named(&config.kitten_tts_2.voice),
        _ => None,
    }
}

impl Speaker for EngineSpeaker {
    fn speak(&self, text: &str) -> Result<Vec<u8>> {
        // A panic in an earlier request must not take speech down for good.
        let mut synthesizer = self.synthesizer.lock().unwrap_or_else(|e| e.into_inner());
        synthesizer.synthesize(&self.config, text, None)
    }

    fn engine(&self) -> String {
        synth::engine_id(&self.config.engine)
    }

    fn voice(&self) -> Option<String> {
        configured_voice(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_voice_reported_is_the_selected_engines() {
        let mut config = TtsConfig { engine: TtsEngine::Piper, voice: "en-us-amy-low".into(), ..Default::default() };
        assert_eq!(configured_voice(&config).as_deref(), Some("en-us-amy-low"));
        config.engine = TtsEngine::Espeak;
        assert_eq!(configured_voice(&config), None);
        config.engine = TtsEngine::PocketTts;
        assert_eq!(configured_voice(&config), Some(config.pocket_tts.voice.clone()));
    }

    #[tokio::test]
    async fn an_engine_the_server_cannot_run_is_refused_at_startup() {
        for engine in [TtsEngine::InflectMicro, TtsEngine::Server] {
            let err = match EngineSpeaker::prepare(TtsConfig { engine, ..Default::default() }).await {
                Ok(_) => panic!("accepted"),
                Err(e) => e,
            };
            assert!(err.to_string().contains("cannot run on the server"), "{err}");
        }
    }

    /// The real thing, end to end, where eSpeak NG is installed.
    #[tokio::test]
    async fn espeak_speaks_a_wav_when_it_is_installed() {
        if voxctrl_config::find_in_path("espeak-ng").is_none() {
            eprintln!("skipped: espeak-ng is not installed");
            return;
        }
        let speaker = EngineSpeaker::prepare(TtsConfig { engine: TtsEngine::Espeak, ..Default::default() })
            .await
            .unwrap();
        let wav = tokio::task::spawn_blocking(move || speaker.speak("Hello from the server.")).await.unwrap().unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(wav.len() > 4000, "only {} bytes of audio", wav.len());
    }
}
