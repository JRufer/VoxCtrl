//! Checking a config for values the app cannot use.

use super::*;

static VALID_MODEL_SIZES: &[&str] = &[
    "tiny", "tiny.en", "base", "base.en", "small", "small.en",
    "medium", "medium.en", "large-v2", "large-v3", "large-v3-turbo",
];

pub fn validate(cfg: &AppConfig) -> Vec<String> {
    let mut errors = Vec::new();

    if !VALID_MODEL_SIZES.contains(&cfg.engine.whisper_cpp.model_size.as_str())
        && !cfg.engine.whisper_cpp.model_size.ends_with(".bin")
        && !std::path::Path::new(&cfg.engine.whisper_cpp.model_size).is_absolute()
    {
        errors.push(format!(
            "Unknown whisper_cpp model_size '{}'. Valid: {:?}",
            cfg.engine.whisper_cpp.model_size, VALID_MODEL_SIZES
        ));
    }

    if !["auto", "cuda", "vulkan", "cpu"]
        .contains(&cfg.engine.whisper_cpp.device.as_str())
    {
        errors.push(format!(
            "Invalid whisper_cpp device '{}'. Use: auto, cuda, vulkan, cpu",
            cfg.engine.whisper_cpp.device
        ));
    }

    if cfg.audio.vad_threshold < 0.0 || cfg.audio.vad_threshold > 1.0 {
        errors.push(format!(
            "vad_threshold {} out of range [0.0, 1.0]",
            cfg.audio.vad_threshold
        ));
    }

    errors
}
