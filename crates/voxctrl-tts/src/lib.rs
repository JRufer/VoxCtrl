//! VoxCtrl text-to-speech engine.
//!
//! Split by concern:
//! - [`piper`]   — Piper voice catalogue, path resolution, binary/voice download
//! - [`pocket`]  — Pocket-TTS (Candle-based neural voice cloning) catalogue + synthesis
//! - [`inflect`] — Inflect-Micro-v2 (ONNX VITS) phoneme frontend + synthesis
//! - [`engine`]  — utterance queue, worker thread, Piper/eSpeak synthesis
//! - [`fifo`]    — named-pipe responder for external speak triggers
//!
//! Snippet expansion and custom-vocabulary correction are shared with
//! `voxctrl-inference` (which applies the same logic to STT output) via the
//! `voxctrl-text` crate.

pub mod breeze;
mod engine;
pub mod hf;
mod fifo;
pub mod inflect;
mod piper;
mod pocket;
pub mod voxcpm;

pub use breeze::{
    breeze_tts_2_model_dir, download_breeze_tts_2_assets, is_breeze_tts_2_ready,
};
pub use voxcpm::{
    download_vox_cpm_2_assets, is_vox_cpm_2_ready, vox_cpm_2_model_dir,
};
pub use voxcpm::is_vox_cpm_2_ready as is_vox_cpm_2_downloaded;
pub use engine::{
    stop_current_playback, ErrorCallback, PlaybackCallback, TtsCommand, TtsEngineHandle,
    TtsEngineWorker, Utterance,
};
pub use fifo::run_fifo_responder;
pub use hf::{
    apply_hf_token, classify_download_error, effective_hf_token, hf_token_from_env,
    looks_like_auth_failure, token_rejected, HF_TOKEN_ENV, HF_TOKEN_REJECTED_TAG,
};
pub use inflect::{
    download_inflect_micro_assets, inflect_micro_model_dir, is_inflect_micro_downloaded,
    INFLECT_MICRO_COMPILED,
};
pub use piper::{
    download_piper_binary, download_voice, get_voice_path, is_voice_downloaded, piper_binary,
    piper_voices_dir, VoiceInfo, PIPER_VOICES,
};
pub use pocket::{
    cloned_tts_voices_dir, download_pocket_tts_assets, is_pocket_tts_ready, pocket_tts_voice,
    pocket_tts_voice_catalogue, PocketTtsVoiceInfo, PocketTtsVoiceOption, POCKET_TTS_VOICES,
};

// Shared with voxctrl-inference, which applies the same logic to STT output.
// See voxctrl-text for the implementation.
pub use voxctrl_text::{correct_custom_vocabulary, expand_snippets};
