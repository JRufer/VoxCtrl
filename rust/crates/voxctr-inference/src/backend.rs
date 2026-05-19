use std::time::Duration;

/// A single transcribed segment or word.
#[derive(Debug, Clone)]
pub struct WordTimestamp {
    pub word: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub probability: f32,
}

/// Full result of a transcription call.
#[derive(Debug, Clone)]
pub struct TranscriptionResult {
    pub text: String,
    pub language: String,
    pub language_probability: f32,
    pub duration_ms: u32,
    pub inference_ms: u32,
    pub word_timestamps: Option<Vec<WordTimestamp>>,
}

/// Configuration passed to each transcribe call.
#[derive(Debug, Clone, Default)]
pub struct TranscribeRequest {
    /// Audio samples at 16 kHz, mono, f32
    pub audio: Vec<f32>,
    pub language: Option<String>,
    pub word_timestamps: bool,
    /// Surrounding text to feed as an initial prompt (improves accuracy)
    pub initial_prompt: Option<String>,
}

/// Common interface every backend must implement.
pub trait TranscriptionBackend: Send + Sync {
    fn name(&self) -> &str;

    /// Load the model. May block for several seconds on first call.
    fn load(&mut self) -> anyhow::Result<()>;

    /// Transcribe a chunk of 16 kHz f32 audio. Blocking.
    fn transcribe(&self, req: &TranscribeRequest) -> anyhow::Result<TranscriptionResult>;

    /// Unload the model to free memory.
    fn unload(&mut self);

    fn is_loaded(&self) -> bool;
}
