//! Synthesis without playback: text in, a WAV file out.
//!
//! The desktop app's worker synthesizes and plays in one step. A server has no
//! sound card to play on; it runs the same engines through [`Synthesizer`] and
//! sends the audio to the client, which plays it.

use std::process::Stdio;

use anyhow::{Context, Result};
use voxctrl_config::{TtsConfig, TtsEngine};

use crate::audiocpp::{self, AudioCppSession};
use crate::piper::{get_voice_path, piper_binary, sample_rate_for_voice_at};
use crate::utterance::Utterance;

/// Whether [`Synthesizer`] can run `engine`. Inflect-Micro streams its audio
/// straight into the playback device and has no buffered form yet.
pub fn supports(engine: &TtsEngine) -> bool {
    !matches!(engine, TtsEngine::InflectMicro | TtsEngine::Server)
}

/// Why [`supports`] says no, for an error message.
fn unsupported(engine: &TtsEngine) -> anyhow::Error {
    match engine {
        TtsEngine::Server => anyhow::anyhow!(
            "The 'server' engine hands speech to another server, which a server cannot do. Choose a real engine."
        ),
        _ => anyhow::anyhow!("Inflect-Micro cannot be synthesized without playback yet. Choose another engine."),
    }
}

/// A short name for the engine, as config files spell it.
pub fn engine_id(engine: &TtsEngine) -> String {
    serde_json::to_value(engine)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{engine:?}").to_lowercase())
}

/// Runs one engine and keeps its model loaded between requests.
#[derive(Default)]
pub struct Synthesizer {
    /// The resident audio.cpp server behind Pocket-TTS, Breeze-TTS-2, VoxCPM2,
    /// Kitten TTS 2 and Reverb-TTS. Piper and eSpeak start a process per request.
    audiocpp: Option<AudioCppSession>,
}

impl Synthesizer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the configured engine's model now, so the first request does not
    /// wait for it. Nothing to do for engines that hold no model.
    pub fn load(&mut self, config: &TtsConfig) -> Result<()> {
        audiocpp::set_audiocpp_server_override(&config.audiocpp_server);
        match config.engine {
            TtsEngine::PocketTts => crate::pocket::ensure_pocket_tts_loaded(config, &mut self.audiocpp),
            TtsEngine::BreezeTts2 => crate::breeze::ensure_breeze_tts_2_loaded(config, &mut self.audiocpp),
            TtsEngine::VoxCpm2 => crate::voxcpm::ensure_vox_cpm_2_loaded(config, &mut self.audiocpp),
            TtsEngine::KittenTts2 => crate::kitten::ensure_kitten_tts_2_loaded(config, &mut self.audiocpp),
            TtsEngine::Reverb => crate::reverb::ensure_reverb_loaded(config, &mut self.audiocpp),
            TtsEngine::Piper | TtsEngine::Espeak | TtsEngine::InflectMicro | TtsEngine::Server => Ok(()),
        }
    }

    /// Speak `text` with the configured engine and return it as a WAV file.
    /// `voice` overrides the configured voice for engines that take one per
    /// request (Piper, Pocket-TTS). Blocking, and can take seconds.
    pub fn synthesize(&mut self, config: &TtsConfig, text: &str, voice: Option<&str>) -> Result<Vec<u8>> {
        let u = Utterance { text: text.to_string(), voice: voice.map(str::to_string), source_label: None };
        audiocpp::set_audiocpp_server_override(&config.audiocpp_server);
        match config.engine {
            TtsEngine::Piper => {
                let (raw, sample_rate) = synth_piper(config, &u)?;
                Ok(pcm16_to_wav(&raw, sample_rate))
            }
            TtsEngine::Espeak => synth_espeak(config, &u),
            TtsEngine::PocketTts => crate::pocket::synth_pocket_tts(config, &u, &mut self.audiocpp),
            TtsEngine::BreezeTts2 => crate::breeze::synth_breeze_tts_2(config, &u, &mut self.audiocpp),
            TtsEngine::VoxCpm2 => crate::voxcpm::synth_vox_cpm_2(config, &u, &mut self.audiocpp),
            TtsEngine::KittenTts2 => crate::kitten::synth_kitten_tts_2(config, &u, &mut self.audiocpp),
            TtsEngine::Reverb => crate::reverb::synth_reverb(config, &u, &mut self.audiocpp),
            TtsEngine::InflectMicro | TtsEngine::Server => Err(unsupported(&config.engine)),
        }
    }
}

/// Download whatever the configured engine and voice need and do not have:
/// the engine's program, its model, the voice. Safe to call on every start.
pub async fn ensure_assets(config: &TtsConfig) -> Result<()> {
    let token = crate::hf::effective_hf_token(config.hf_token.as_deref());
    audiocpp::set_audiocpp_server_override(&config.audiocpp_server);
    match config.engine {
        TtsEngine::Piper => {
            if piper_binary().is_none() {
                crate::piper::download_piper_binary().await.context("downloading the Piper program")?;
            }
            if !crate::piper::is_voice_downloaded(&config.voice, &config.voice_dir) {
                crate::piper::download_voice(&config.voice, &config.voice_dir)
                    .await
                    .with_context(|| format!("downloading the Piper voice '{}'", config.voice))?;
            }
            Ok(())
        }
        TtsEngine::Espeak => {
            if voxctrl_config::find_in_path("espeak-ng").is_none() {
                anyhow::bail!("espeak-ng is not installed. Install it with the system's package manager.");
            }
            Ok(())
        }
        TtsEngine::PocketTts => {
            audiocpp::ensure_audiocpp_runtime().await.context("downloading the audio.cpp runtime")?;
            let cfg = &config.pocket_tts;
            if !crate::pocket::is_pocket_tts_ready(&cfg.voice, &cfg.voice_dir) {
                crate::pocket::download_pocket_tts_assets(&cfg.voice, &cfg.voice_dir, token)
                    .await
                    .context("downloading Pocket-TTS")?;
            }
            Ok(())
        }
        TtsEngine::BreezeTts2 => {
            audiocpp::ensure_audiocpp_runtime().await.context("downloading the audio.cpp runtime")?;
            if !crate::breeze::is_breeze_tts_2_ready(&config.breeze_tts_2.model_dir) {
                crate::breeze::download_breeze_tts_2_assets(&config.breeze_tts_2.model_dir, token)
                    .await
                    .context("downloading Breeze-TTS-2")?;
            }
            Ok(())
        }
        TtsEngine::VoxCpm2 => {
            audiocpp::ensure_audiocpp_runtime().await.context("downloading the audio.cpp runtime")?;
            if !crate::voxcpm::is_vox_cpm_2_ready(&config.vox_cpm_2.model_dir) {
                crate::voxcpm::download_vox_cpm_2_assets(&config.vox_cpm_2.model_dir, token)
                    .await
                    .context("downloading VoxCPM2")?;
            }
            Ok(())
        }
        TtsEngine::KittenTts2 => {
            audiocpp::ensure_audiocpp_runtime().await.context("downloading the audio.cpp runtime")?;
            if !crate::kitten::is_kitten_tts_2_ready(&config.kitten_tts_2.model_dir) {
                crate::kitten::download_kitten_tts_2_assets(&config.kitten_tts_2.model_dir, token)
                    .await
                    .context("downloading Kitten TTS 2")?;
            }
            Ok(())
        }
        TtsEngine::Reverb => {
            audiocpp::ensure_audiocpp_runtime().await.context("downloading the audio.cpp runtime")?;
            if !crate::reverb::is_reverb_ready(&config.reverb.model_dir) {
                crate::reverb::download_reverb_assets(&config.reverb.model_dir, token)
                    .await
                    .context("downloading Reverb-TTS")?;
            }
            Ok(())
        }
        TtsEngine::InflectMicro | TtsEngine::Server => Err(unsupported(&config.engine)),
    }
}

/// Run Piper on the utterance. Returns its raw output (16-bit mono PCM) and
/// the voice's sample rate.
pub(crate) fn synth_piper(config: &TtsConfig, u: &Utterance) -> Result<(Vec<u8>, u32)> {
    let binary = piper_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "Piper binary not found. Download a voice from TTS settings (this \
             also installs the standalone Piper engine), or install piper \
             system-wide."
        )
    })?;
    let voice_name = u.voice.as_deref().unwrap_or(&config.voice);

    let voice_path = get_voice_path(voice_name, &config.voice_dir)
        .ok_or_else(|| anyhow::anyhow!("Piper voice files not found for: {}", voice_name))?;

    let length_scale = 1.0 / config.speed;
    let mut cmd = std::process::Command::new(&binary);
    cmd.arg("--model").arg(&voice_path).arg("--length-scale").arg(length_scale.to_string()).arg("--output-raw");

    if config.gpu {
        cmd.arg("--cuda");
    }

    let mut piper =
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().context("spawn piper")?;

    use std::io::Write;
    piper.stdin.as_mut().unwrap().write_all(u.text.as_bytes()).context("write to piper stdin")?;

    let output = piper.wait_with_output().context("wait piper")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("piper process failed with exit code {:?}: {}", output.status.code(), err_msg.trim());
    }

    if output.stdout.is_empty() {
        anyhow::bail!("piper produced empty stdout");
    }

    // Rate from the voice's own config where it has one, so a voice the
    // built-in catalogue does not know still plays at its real pitch.
    let sample_rate = sample_rate_for_voice_at(voice_name, &voice_path);
    Ok((output.stdout, sample_rate))
}

/// Run eSpeak NG and capture the WAV it would otherwise play.
fn synth_espeak(config: &TtsConfig, u: &Utterance) -> Result<Vec<u8>> {
    if voxctrl_config::find_in_path("espeak-ng").is_none() {
        anyhow::bail!("espeak-ng is not installed on this system. Install it with your package manager.");
    }
    let speed = if config.speed <= 0.0 { 1.0 } else { config.speed };
    let wpm = (175.0 * speed) as i32;
    let output = std::process::Command::new("espeak-ng")
        .arg("-s")
        .arg(wpm.to_string())
        .arg("--stdout")
        // After `--`, so text starting with a dash is not read as an option.
        .arg("--")
        .arg(&u.text)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .context("spawn espeak-ng")?;
    if !output.status.success() {
        anyhow::bail!(
            "espeak-ng exited with status {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if output.stdout.is_empty() {
        anyhow::bail!("espeak-ng produced no audio");
    }
    Ok(output.stdout)
}

/// Wrap 16-bit little-endian mono PCM in a WAV header.
pub fn pcm16_to_wav(raw: &[u8], sample_rate: u32) -> Vec<u8> {
    // A trailing odd byte is half a sample; drop it.
    let data = &raw[..raw.len() - raw.len() % 2];
    let data_len = data.len() as u32;
    let mut wav = Vec::with_capacity(44 + data.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // bytes per second
    wav.extend_from_slice(&2u16.to_le_bytes()); // bytes per sample frame
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(data);
    wav
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_is_wrapped_in_a_header_a_player_can_read() {
        let raw: Vec<u8> = [0i16, 1000, -1000, 32767].iter().flat_map(|s| s.to_le_bytes()).collect();
        let wav = pcm16_to_wav(&raw, 22050);
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 1, "mono");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 22050);
        assert_eq!(u32::from_le_bytes(wav[28..32].try_into().unwrap()), 44100, "bytes per second");
        assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()), 16);
        assert_eq!(&wav[36..40], b"data");
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(&wav[44..], &raw[..]);
    }

    #[test]
    fn half_a_trailing_sample_is_dropped() {
        let wav = pcm16_to_wav(&[1, 2, 3], 16000);
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 2);
        assert_eq!(wav.len(), 46);
    }

    #[cfg(feature = "playback")]
    #[test]
    fn the_wav_decodes_at_the_right_rate() {
        use rodio::Source;
        let raw: Vec<u8> = (0..1600i16).flat_map(|s| s.to_le_bytes()).collect();
        let decoded = rodio::Decoder::new(std::io::Cursor::new(pcm16_to_wav(&raw, 16000))).unwrap();
        assert_eq!(decoded.sample_rate(), 16000);
        assert_eq!(decoded.channels(), 1);
        assert_eq!(decoded.count(), 1600);
    }

    #[test]
    fn engines_are_named_as_config_files_spell_them() {
        assert_eq!(engine_id(&TtsEngine::Piper), "piper");
        assert_eq!(engine_id(&TtsEngine::KittenTts2), "kitten_tts_2");
        assert_eq!(engine_id(&TtsEngine::Reverb), "reverb");
    }

    #[test]
    fn only_inflect_micro_and_the_server_engine_are_unsupported() {
        assert!(!supports(&TtsEngine::InflectMicro));
        assert!(!supports(&TtsEngine::Server));
        let config = TtsConfig { engine: TtsEngine::Server, ..Default::default() };
        let err = Synthesizer::new().synthesize(&config, "hello", None).unwrap_err();
        assert!(err.to_string().contains("another server"), "{err}");
        for engine in [
            TtsEngine::Piper,
            TtsEngine::Espeak,
            TtsEngine::PocketTts,
            TtsEngine::BreezeTts2,
            TtsEngine::VoxCpm2,
            TtsEngine::KittenTts2,
            TtsEngine::Reverb,
        ] {
            assert!(supports(&engine), "{engine:?}");
        }
    }

    #[test]
    fn inflect_micro_is_refused_with_a_reason() {
        let config = TtsConfig { engine: TtsEngine::InflectMicro, ..Default::default() };
        let err = Synthesizer::new().synthesize(&config, "hello", None).unwrap_err();
        assert!(err.to_string().contains("Inflect-Micro"), "{err}");
    }
}
