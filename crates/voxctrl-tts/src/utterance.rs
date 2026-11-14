//! What gets spoken, and the callbacks around speaking it. Shared by the
//! playback worker and by synthesis without playback (the server).

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,
    pub source_label: Option<String>,
}

pub type PlaybackCallback = Arc<dyn Fn() + Send + Sync + 'static>;
/// Called with a human-readable message whenever an utterance fails to play
/// (engine missing, voice not downloaded, audio device unavailable, ...).
pub type ErrorCallback = Arc<dyn Fn(String) + Send + Sync + 'static>;
