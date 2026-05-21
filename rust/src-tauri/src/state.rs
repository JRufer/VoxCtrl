use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

use tokio::sync::Mutex;
use voxctr_config::Config;
use voxctr_routing::OutputTargetRouter;

/// All shared mutable state, behind Arc so it can be handed to Tauri commands.
pub struct AppState {
    pub config: Arc<Mutex<Config>>,
    pub router: Arc<Mutex<OutputTargetRouter>>,

    /// True while a hotkey hold/toggle is active (recording)
    pub recording: Arc<AtomicBool>,
    /// True while TTS is playing back
    pub speaking: Arc<AtomicBool>,

    /// Total words injected this session
    pub word_count: Arc<std::sync::atomic::AtomicU32>,

    /// Most recent transcription result (shown in history + overlay)
    pub last_text: Arc<Mutex<String>>,

    /// Currently active dictation target ID
    pub active_target: Arc<Mutex<String>>,

    /// Currently configured target definitions (in-memory cache for fast lookups)
    pub targets: Arc<Mutex<Vec<voxctr_routing::OutputTarget>>>,

    /// Transcript history — most recent first
    pub history: Arc<Mutex<Vec<HistoryEntry>>>,

    /// Channel sender to send empty audio chunks as sentinels to unblock the coordinator thread
    pub audio_tx: crossbeam_channel::Sender<Vec<f32>>,

    /// TTS playback engine handle
    pub tts_handle: Arc<Mutex<Option<voxctr_tts::TtsEngineHandle>>>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct HistoryEntry {
    pub text: String,
    pub target_id: String,
    pub timestamp: String,
    pub inference_ms: u32,
}

impl AppState {
    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    pub fn is_speaking(&self) -> bool {
        self.speaking.load(Ordering::SeqCst)
    }

    pub fn set_recording(&self, v: bool) {
        self.recording.store(v, Ordering::SeqCst);
        if !v {
            let _ = self.audio_tx.send(Vec::new());
        }
    }

    pub fn set_speaking(&self, v: bool) {
        self.speaking.store(v, Ordering::SeqCst);
    }

    pub fn increment_words(&self, n: u32) {
        self.word_count.fetch_add(n, Ordering::SeqCst);
    }

    pub fn total_words(&self) -> u32 {
        self.word_count.load(Ordering::SeqCst)
    }
}
