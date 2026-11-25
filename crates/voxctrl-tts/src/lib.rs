//! VoxCtrl text-to-speech engine.
//!
//! With the default `playback` feature this is the app's speaking voice: a
//! worker thread that synthesizes and plays through the sound card. Without
//! it (the server) only synthesis is built, through [`synth`], and no audio
//! library is linked.
//!
//! Split by concern:
//! - [`piper`]    — Piper voice catalogue, path resolution, binary/voice download
//! - [`audiocpp`] — shared audio.cpp runtime: binary management, GGUF asset
//!   download, subprocess synthesis (Pocket-TTS, Breeze-TTS-2, VoxCPM2,
//!   Kitten TTS 2, Reverb-TTS)
//! - [`pocket`]   — Pocket-TTS voice catalogue + audio.cpp synthesis
//! - [`breeze`]   — Breeze-TTS-2 asset management + audio.cpp synthesis
//! - [`voxcpm`]   — VoxCPM2 asset management + audio.cpp synthesis
//! - [`kitten`]   — Kitten TTS 2 asset management + audio.cpp synthesis
//! - [`reverb`]   — Reverb-TTS asset management + audio.cpp synthesis
//! - [`inflect`]  — Inflect-Micro-v2 (ONNX VITS) phoneme frontend + synthesis
//! - [`engine`]   — utterance queue, worker thread, Piper/eSpeak synthesis
//! - [`fifo`]     — named-pipe responder for external speak triggers
//!
//! Snippet expansion and custom-vocabulary correction are shared with
//! `voxctrl-inference` (which applies the same logic to STT output) via the
//! `voxctrl-text` crate.

// Without playback, the worker that calls most of the model-loading helpers
// is not built; they are still part of the engines.
#![cfg_attr(not(feature = "playback"), allow(dead_code, unused_imports))]

mod audiocpp;
pub mod breeze;
#[cfg(feature = "playback")]
mod engine;
#[cfg(feature = "playback")]
mod fifo;
pub mod hf;
pub mod inflect;
pub mod kitten;
mod piper;
mod pocket;
pub mod remote;
pub mod reverb;
pub mod synth;
mod utterance;
pub mod voxcpm;

pub use audiocpp::{set_audiocpp_server_override, set_helper_niceness};
pub use breeze::{breeze_tts_2_model_dir, download_breeze_tts_2_assets, is_breeze_tts_2_ready};
#[cfg(feature = "playback")]
pub use engine::{stop_current_playback, TtsCommand, TtsEngineHandle, TtsEngineWorker};
#[cfg(feature = "playback")]
pub use fifo::run_fifo_responder;
pub use hf::{
    apply_hf_token, classify_download_error, effective_hf_token, hf_token_from_env, looks_like_auth_failure,
    token_rejected, HF_TOKEN_ENV, HF_TOKEN_REJECTED_TAG,
};
pub use inflect::{
    download_inflect_micro_assets, inflect_micro_model_dir, is_inflect_micro_downloaded, INFLECT_MICRO_COMPILED,
};
pub use kitten::{
    download_kitten_tts_2_assets, is_kitten_tts_2_ready, kitten_tts_2_model_dir, KITTEN_TTS_2_LICENSE_NOTE,
    KITTEN_TTS_2_VOICES,
};
pub use piper::{
    download_piper_binary, download_voice, get_voice_path, is_voice_downloaded, piper_binary, piper_voice_catalogue,
    piper_voices_dir, PiperVoiceOption, VoiceInfo, PIPER_VOICES,
};
pub use pocket::{
    cloned_tts_voices_dir, download_pocket_tts_assets, is_pocket_tts_ready, pocket_tts_voice,
    pocket_tts_voice_catalogue, PocketTtsVoiceInfo, PocketTtsVoiceOption, POCKET_TTS_VOICES,
};
pub use reverb::{
    download_reverb_assets, is_reverb_ready, reverb_model_dir, reverb_runtime_supported, REVERB_LANGUAGES,
    REVERB_LICENSE_NOTE, REVERB_VOICES,
};
pub use utterance::{ErrorCallback, PlaybackCallback, Utterance};
pub use voxcpm::is_vox_cpm_2_ready as is_vox_cpm_2_downloaded;
pub use voxcpm::{download_vox_cpm_2_assets, is_vox_cpm_2_ready, vox_cpm_2_model_dir};

// Shared with voxctrl-inference, which applies the same logic to STT output.
// See voxctrl-text for the implementation.
pub use voxctrl_text::{correct_custom_vocabulary, expand_snippets};

/// Bring VoxCtrl's own audio.cpp runtime up to date if an update has left it
/// behind, when the configured engine is one that runs on it. For app
/// startup: someone whose models are already downloaded would otherwise keep
/// an out-of-date runtime until they next downloaded a model.
///
/// A missing runtime is left missing (it is installed with a model), and one
/// the user installed system-wide is never touched.
pub async fn update_audiocpp_runtime_if_outdated(config: &voxctrl_config::TtsConfig) -> anyhow::Result<bool> {
    use voxctrl_config::TtsEngine;
    let uses_audiocpp = matches!(
        config.engine,
        TtsEngine::PocketTts | TtsEngine::BreezeTts2 | TtsEngine::VoxCpm2 | TtsEngine::KittenTts2 | TtsEngine::Reverb
    );
    if !uses_audiocpp || audiocpp::audiocpp_runtime() != audiocpp::AudioCppRuntime::Outdated {
        return Ok(false);
    }
    audiocpp::ensure_audiocpp_runtime().await?;
    Ok(true)
}
