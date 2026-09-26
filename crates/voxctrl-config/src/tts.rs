//! Text-to-speech settings, per engine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsEngine {
    #[default]
    Piper,
    Espeak,
    PocketTts,
    InflectMicro,
    #[serde(rename = "breeze_tts_2", alias = "breeze_tts2")]
    BreezeTts2,
    #[serde(rename = "vox_cpm_2", alias = "voxcpm2", alias = "vox_cpm2")]
    VoxCpm2,
}

/// How the TTS engine manages the memory of model-backed engines (currently
/// Inflect-Micro-v2; Piper, eSpeak, and the audio.cpp-backed engines
/// (Pocket-TTS, Breeze-TTS-2, VoxCPM2) all shell out to a subprocess per
/// utterance and hold nothing in-process).
///
/// * `AlwaysLoaded` — once loaded, the model stays resident for the lifetime of
///   the TTS worker. Fastest, but the weights sit in RAM even when unused.
/// * `OnDemand` — the model is loaded the moment VoxCtrl knows it will be
///   needed, kept "primed" while it keeps getting used, and dropped again after
///   [`TtsConfig::idle_unload_secs`] of inactivity to give the memory back.
#[derive(Default, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TtsMemoryMode {
    /// The default: existing installs keep the behaviour they already had;
    /// opting into the memory saving is a deliberate choice (it costs
    /// first-word latency).
    #[default]
    AlwaysLoaded,
    OnDemand,
}

/// 15 minutes — long enough that a back-and-forth conversation never pays the
/// reload cost twice, short enough that an idle session gives the RAM back.
pub const DEFAULT_TTS_IDLE_UNLOAD_SECS: u64 = 900;

fn default_tts_idle_unload_secs() -> u64 {
    DEFAULT_TTS_IDLE_UNLOAD_SECS
}

fn default_pocket_tts_voice() -> String {
    "alba".into()
}

fn default_breeze_tts_2_speaker_prompt() -> String {
    "A calm and clear female voice speaking at a natural pace".into()
}

fn default_tts_speed() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PocketTtsConfig {
    /// Bundled reference voice name, e.g. "alba", "anna", "vera", "charles", "michael",
    /// or the filename stem of a custom clip dropped into `voice_dir`.
    #[serde(default = "default_pocket_tts_voice")]
    pub voice: String,
    /// Pre-warm model on startup so the first synthesis is instant
    #[serde(default)]
    pub prewarm: bool,
    /// Where this engine's token used to live, kept only so a config written
    /// before `tts.hf_token` existed can be migrated. Cleared once lifted, and
    /// never written back.
    #[serde(default, rename = "hf_token", skip_serializing_if = "Option::is_none")]
    pub legacy_hf_token: Option<String>,
    /// Directory scanned for custom voice clips (`<id>.wav`). Empty = platform default
    /// (`~/.local/share/voxctrl/cloned-tts-voices/`).
    #[serde(default)]
    pub voice_dir: String,
    /// Enable Vulkan GPU acceleration (via audio.cpp's `--backend vulkan`).
    /// Falls back to CPU whenever no usable Vulkan device can be opened.
    #[serde(default)]
    pub gpu: bool,
}

impl Default for PocketTtsConfig {
    fn default() -> Self {
        Self {
            voice: default_pocket_tts_voice(),
            prewarm: false,
            gpu: false,
            legacy_hf_token: None,
            voice_dir: String::new(),
        }
    }
}

/// Breeze-TTS-2 (BreezeBlue) — bilingual neural text-to-speech with natural-language
/// voice design speaker prompts. Model weights are gated on HuggingFace under a
/// non-commercial research license.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreezeTts2Config {
    /// Voice selection mode: "prompt" (Voice Design) or "clone" (Cloned Voice Clip)
    #[serde(default = "default_breeze_voice_mode")]
    pub voice_mode: String,
    /// Selected cloned voice ID from the shared voice folder (e.g. "alba", "my_voice")
    #[serde(default = "default_breeze_tts_2_cloned_voice")]
    pub cloned_voice: String,
    /// Shared voice directory for custom clips (empty = platform default `~/.local/share/voxctrl/cloned-tts-voices/`)
    #[serde(default)]
    pub voice_dir: String,
    /// Text prompt describing the voice of the speaker (Voice Design)
    #[serde(default = "default_breeze_tts_2_speaker_prompt")]
    pub speaker_prompt: String,
    /// Directory containing model weights & tokenizer. Empty = platform default
    /// (`~/.local/share/voxctrl/models/breeze-tts-2/`).
    #[serde(default)]
    pub model_dir: String,
    /// Where this engine's token used to live; see
    /// [`PocketTtsConfig::legacy_hf_token`].
    #[serde(default, rename = "hf_token", skip_serializing_if = "Option::is_none")]
    pub legacy_hf_token: Option<String>,
    /// Pre-warm model on startup so the first synthesis is instant
    #[serde(default)]
    pub prewarm: bool,
    /// Enable Vulkan GPU acceleration (via audio.cpp's `--backend vulkan`). Falls
    /// back to CPU whenever no usable Vulkan device can be opened.
    #[serde(default)]
    pub gpu: bool,
}

fn default_breeze_voice_mode() -> String {
    "prompt".into()
}

fn default_breeze_tts_2_cloned_voice() -> String {
    "alba".into()
}

impl Default for BreezeTts2Config {
    fn default() -> Self {
        Self {
            voice_mode: default_breeze_voice_mode(),
            cloned_voice: default_breeze_tts_2_cloned_voice(),
            voice_dir: String::new(),
            speaker_prompt: default_breeze_tts_2_speaker_prompt(),
            model_dir: String::new(),
            legacy_hf_token: None,
            prewarm: false,
            gpu: false,
        }
    }
}

/// VoxCPM2 — neural text-to-speech with natural-language voice design speaker
/// prompts and reference voice cloning (including Ultimate Cloning with paired audio + transcript).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoxCpm2Config {
    /// Voice selection mode: "prompt" (Voice Design) or "clone" (Voice Cloning)
    #[serde(default = "default_vox_cpm_2_voice_mode")]
    pub voice_mode: String,
    /// Selected cloned voice ID from the shared voice folder (e.g. "alba", "my_voice")
    #[serde(default = "default_vox_cpm_2_cloned_voice")]
    pub cloned_voice: String,
    /// Shared voice directory for custom clips (empty = platform default `~/.local/share/voxctrl/cloned-tts-voices/`)
    #[serde(default)]
    pub voice_dir: String,
    /// Text prompt describing speaker characteristics (for Voice Design)
    #[serde(default = "default_vox_cpm_2_speaker_prompt")]
    pub speaker_prompt: String,
    /// Enable Ultimate Cloning (requires reference audio + transcript in the same folder)
    #[serde(default)]
    pub ultimate_cloning: bool,
    /// Directory containing downloaded model weights & tokenizer. Empty = platform default
    /// (`~/.local/share/voxctrl/models/voxcpm2/`).
    #[serde(default)]
    pub model_dir: String,
    /// Pre-warm model on startup so first synthesis is instant
    #[serde(default)]
    pub prewarm: bool,
    /// Enable Vulkan GPU acceleration (via audio.cpp's `--backend vulkan`). Falls
    /// back to CPU whenever no usable Vulkan device can be opened.
    #[serde(default)]
    pub gpu: bool,
}

fn default_vox_cpm_2_voice_mode() -> String {
    "prompt".into()
}

fn default_vox_cpm_2_cloned_voice() -> String {
    "alba".into()
}

fn default_vox_cpm_2_speaker_prompt() -> String {
    "A calm young female voice speaking clearly with a gentle tone.".into()
}

impl Default for VoxCpm2Config {
    fn default() -> Self {
        Self {
            voice_mode: default_vox_cpm_2_voice_mode(),
            cloned_voice: default_vox_cpm_2_cloned_voice(),
            voice_dir: String::new(),
            speaker_prompt: default_vox_cpm_2_speaker_prompt(),
            ultimate_cloning: false,
            model_dir: String::new(),
            prewarm: false,
            gpu: false,
        }
    }
}

fn default_inflect_micro_seed() -> u64 {
    0
}

fn default_inflect_micro_noise_scale() -> f32 {
    0.667
}

/// Inflect-Micro-v2 (<https://huggingface.co/owensong/Inflect-Micro-v2>) — a
/// ~9.4M-parameter VITS-family model with a single fixed English voice, so
/// unlike Piper and Pocket-TTS there is no voice to pick. Speaking rate comes
/// from the shared [`TtsConfig::speed`]; what remains model-specific is the
/// sampling seed and the two VITS noise scales.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InflectMicroConfig {
    /// Directory holding the ONNX graphs and phoneme vocabulary. Empty = platform
    /// default (`~/.local/share/voxctrl/models/inflect-micro/`).
    #[serde(default)]
    pub model_dir: String,
    /// Seed for the stochastic duration predictor and latent sampling. The model
    /// is deterministic for a fixed seed, so a stable value keeps repeated
    /// synthesis of the same text identical.
    #[serde(default = "default_inflect_micro_seed")]
    pub seed: u64,
    /// Latent sampling temperature, fed to `decode.onnx` as `noise_scale`.
    /// Higher is more varied, lower is flatter. Valid range is 0.0 to 1.0.
    #[serde(default = "default_inflect_micro_noise_scale")]
    pub noise_scale: f32,
    /// Pre-warm the ONNX sessions on startup so the first synthesis is instant.
    #[serde(default)]
    pub prewarm: bool,
}

impl Default for InflectMicroConfig {
    fn default() -> Self {
        Self {
            model_dir: String::new(),
            seed: default_inflect_micro_seed(),
            noise_scale: default_inflect_micro_noise_scale(),
            prewarm: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub enabled: bool,
    pub engine: TtsEngine,
    /// Voice name for Piper, e.g. "en-us-lessac-medium"
    pub voice: String,
    /// Directory containing Piper voice files. Empty = platform default.
    #[serde(default)]
    pub voice_dir: String,
    /// Key(s) that stop TTS playback, e.g. ["KEY_ESC"]
    pub stop_key: Vec<String>,
    pub response_overlay: bool,
    #[serde(default = "default_tts_speed")]
    pub speed: f32,
    /// Enable GPU acceleration for Piper
    #[serde(default)]
    pub gpu: bool,
    /// The single HuggingFace access token for every gated model VoxCtrl
    /// downloads (Pocket-TTS and Breeze-TTS-2 today). One token, one place —
    /// entering it in Settings or in the setup wizard writes here.
    #[serde(default)]
    pub hf_token: Option<String>,
    #[serde(default)]
    pub pocket_tts: PocketTtsConfig,
    /// Whether a model-backed engine stays resident or is unloaded when idle.
    #[serde(default)]
    pub memory_mode: TtsMemoryMode,
    /// Idle time before the model is unloaded in [`TtsMemoryMode::OnDemand`].
    /// The countdown restarts every time the model is used or pre-loaded.
    #[serde(default = "default_tts_idle_unload_secs")]
    pub idle_unload_secs: u64,
    #[serde(default)]
    pub inflect_micro: InflectMicroConfig,
    #[serde(default)]
    pub breeze_tts_2: BreezeTts2Config,
    #[serde(default)]
    pub vox_cpm_2: VoxCpm2Config,
    #[serde(default)]
    pub snippets: std::collections::HashMap<String, String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // eSpeak-NG is a system package installed by the setup flow
            // (installer.rs) — it works immediately with no model download,
            // unlike Piper (needs a voice download) or Pocket-TTS (needs a
            // multi-hundred-MB gated model download).
            engine: TtsEngine::Espeak,
            voice: "en-us-lessac-medium".into(),
            voice_dir: String::new(),
            stop_key: vec!["KEY_ESC".into()],
            response_overlay: true,
            speed: 1.0,
            gpu: false,
            hf_token: None,
            pocket_tts: PocketTtsConfig::default(),
            memory_mode: TtsMemoryMode::default(),
            idle_unload_secs: DEFAULT_TTS_IDLE_UNLOAD_SECS,
            inflect_micro: InflectMicroConfig::default(),
            breeze_tts_2: BreezeTts2Config::default(),
            vox_cpm_2: VoxCpm2Config::default(),
            snippets: {
                let mut map = std::collections::HashMap::new();
                map.insert("VoxCtrl".into(), "Voks Con-trol".into());
                map.insert("voxctrl".into(), "Voks Con-trol".into());
                map.insert("Vox Control".into(), "Voks Con-trol".into());
                map
            },
        }
    }
}

impl TtsConfig {
    /// True when the model should be dropped after an idle period.
    pub fn unloads_when_idle(&self) -> bool {
        self.memory_mode == TtsMemoryMode::OnDemand
    }

    /// Idle window before unloading, clamped to a sane range. A zero or absurdly
    /// small value would thrash the loader (the model would be dropped between
    /// two sentences of the same reply), so the floor is 30s.
    pub fn idle_unload_duration(&self) -> std::time::Duration {
        const MIN_SECS: u64 = 30;
        std::time::Duration::from_secs(self.idle_unload_secs.max(MIN_SECS))
    }
}
