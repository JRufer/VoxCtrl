//! What happens to a transcript between the speech engine and delivery: the
//! noise gate, the recognition prompt, text post-processing, the silence
//! hallucination filter and the per-hotkey OpenAI rewrite settings.
//!
//! Both transcription paths use this — the local worker in this crate and the
//! remote streaming session finished in the app's pipeline — so a tuning
//! change or a fix lands in one place and the two cannot drift apart.

use voxctrl_config::{AppConfig, OpenAiConfig, OpenAiMode};
use voxctrl_routing::{HotkeyBinding, OutputTarget};

use crate::postprocess::{is_silence_hallucination, run_pipeline, PostProcessConfig};

/// Below this RMS the room is silent, so a stock hallucination ("Thank you.")
/// is discarded rather than delivered. A genuinely spoken "thank you" is far
/// louder than this.
pub const SILENCE_RMS: f32 = 0.003;

/// The recognition prompt every engine that accepts one is given.
const BASE_PROMPT: &str =
    "VoxCtrl is a voice control assistant application. VoxCtrl commands start with Vox Control or Hey Vox.";

/// Root-mean-square energy of `audio`; 0 for an empty buffer.
pub fn rms(audio: &[f32]) -> f32 {
    if audio.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = audio.iter().map(|&s| s * s).sum();
    (sum_sq / audio.len() as f32).sqrt()
}

/// The RMS below which a recording is dropped by the noise gate.
///
/// `vad_threshold` is a sensitivity (0–1): 1.0 opens the gate completely, 0.0
/// sets the highest gate (0.006 RMS). The default 0.5 maps to 0.003, which lets
/// speech through while filtering silence.
pub fn noise_gate_threshold(vad_threshold: f32) -> f32 {
    (1.0 - vad_threshold) * 0.006
}

/// The initial prompt for recognition: the trigger phrases plus the user's
/// custom vocabulary, so both are spelled the way the user expects.
pub fn initial_prompt(custom_vocabulary: &[String]) -> String {
    if custom_vocabulary.is_empty() {
        BASE_PROMPT.to_string()
    } else {
        format!("{BASE_PROMPT} Vocabulary: {}.", custom_vocabulary.join(", "))
    }
}

/// The first id of a comma-separated target list, which decides the
/// processing overrides for the whole utterance.
fn primary_target_id(target_id: &str) -> &str {
    target_id
        .split(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .unwrap_or("default")
}

/// Post-processing settings for `target_id`: the target's own overrides where
/// it has them, the global feature settings otherwise.
pub fn post_process_config<'a>(
    target_id: &str,
    app: &'a AppConfig,
    targets: &[OutputTarget],
) -> PostProcessConfig<'a> {
    let primary = primary_target_id(target_id);
    let processing = targets.iter().find(|t| t.id == primary).map(|t| &t.processing);
    let features = &app.features;

    PostProcessConfig {
        remove_fillers: processing
            .and_then(|p| p.remove_fillers)
            .unwrap_or(features.remove_fillers),
        spoken_punctuation: processing
            .and_then(|p| p.spoken_punctuation)
            .unwrap_or(features.spoken_punctuation),
        auto_format_lists: processing
            .and_then(|p| p.auto_format_lists)
            .unwrap_or(features.auto_format_lists),
        // Snippets always expand; the only thing that turns them off is having
        // none defined.
        apply_snippets: !features.snippets.is_empty(),
        snippets: &features.snippets,
        code_mode: processing.and_then(|p| p.code_mode).unwrap_or(false),
        custom_vocabulary: &features.custom_vocabulary,
    }
}

/// Run the text pipeline over a raw transcript, then drop it if it is a known
/// silence hallucination and the audio (energy `rms`) really was silence.
pub fn post_process(
    raw_text: &str,
    rms: f32,
    target_id: &str,
    app: &AppConfig,
    targets: &[OutputTarget],
) -> String {
    let processed = run_pipeline(raw_text, &post_process_config(target_id, app, targets));
    if !processed.is_empty() && rms < SILENCE_RMS && is_silence_hallucination(&processed) {
        tracing::info!("Discarded silence hallucination '{processed}' (audio RMS: {rms:.5})");
        return String::new();
    }
    processed
}

/// The OpenAI settings a hotkey's rewrite runs with, or `None` when the
/// binding does not ask for one.
///
/// The global settings are the base; the binding may override the model, pick
/// a preset (which replaces the system prompt), give its own system prompt
/// (which wins over the preset) and its own user-prompt template.
pub fn binding_openai_config(
    binding: Option<&HotkeyBinding>,
    base: &OpenAiConfig,
) -> Option<OpenAiConfig> {
    let binding = binding.filter(|b| b.openai_enabled == Some(true))?;
    let non_empty = |s: &Option<String>| s.as_deref().filter(|s| !s.is_empty()).map(str::to_string);

    let mut cfg = base.clone();
    cfg.enabled = true;
    if let Some(model) = non_empty(&binding.openai_model) {
        cfg.model = model;
    }
    if let Some(ref key) = binding.openai_mode {
        // An unknown preset name falls back to the default, as it always has.
        let mode = OpenAiMode::from_key(key).unwrap_or_default();
        if mode != OpenAiMode::Custom {
            cfg.system_prompt = voxctrl_llm::preset_system_prompt(&mode).to_string();
            cfg.mode = mode;
        }
    }
    if let Some(system_prompt) = non_empty(&binding.openai_system_prompt) {
        cfg.system_prompt = system_prompt;
    }
    if let Some(prompt) = non_empty(&binding.openai_prompt) {
        cfg.user_prompt = prompt;
    }
    Some(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> HotkeyBinding {
        serde_json::from_value(serde_json::json!({
            "id": "b", "keys": ["KEY_F9"], "gesture": "hold", "target_id": "default",
        }))
        .unwrap()
    }

    #[test]
    fn noise_gate_maps_sensitivity_to_rms() {
        assert_eq!(noise_gate_threshold(1.0), 0.0);
        assert!((noise_gate_threshold(0.5) - 0.003).abs() < 1e-9);
        assert!((noise_gate_threshold(0.0) - 0.006).abs() < 1e-9);
    }

    #[test]
    fn rms_of_nothing_is_zero() {
        assert_eq!(rms(&[]), 0.0);
        assert_eq!(rms(&[0.5, -0.5]), 0.5);
    }

    #[test]
    fn prompt_carries_the_vocabulary_only_when_there_is_one() {
        assert_eq!(initial_prompt(&[]), BASE_PROMPT);
        assert_eq!(
            initial_prompt(&["Tauri".into(), "Svelte".into()]),
            format!("{BASE_PROMPT} Vocabulary: Tauri, Svelte.")
        );
    }

    #[test]
    fn target_overrides_come_from_the_first_listed_target() {
        let mut app = AppConfig::default();
        app.features.remove_fillers = false;
        let mut target: OutputTarget = serde_json::from_value(serde_json::json!({
            "id": "notes", "label": "Notes", "delivery": "inject",
        }))
        .unwrap();
        target.processing.remove_fillers = Some(true);
        let targets = [target];

        assert!(post_process_config(" notes , other", &app, &targets).remove_fillers);
        assert!(!post_process_config("other,notes", &app, &targets).remove_fillers);
    }

    #[test]
    fn a_binding_without_openai_gets_no_rewrite() {
        assert!(binding_openai_config(None, &OpenAiConfig::default()).is_none());
        assert!(binding_openai_config(Some(&binding()), &OpenAiConfig::default()).is_none());
    }

    #[test]
    fn binding_overrides_layer_over_the_global_settings() {
        let mut b = binding();
        b.openai_enabled = Some(true);
        b.openai_mode = Some("formal".into());
        let cfg = binding_openai_config(Some(&b), &OpenAiConfig::default()).unwrap();
        assert!(cfg.enabled);
        assert_eq!(cfg.mode, OpenAiMode::Formal);
        assert_eq!(cfg.system_prompt, voxctrl_llm::preset_system_prompt(&OpenAiMode::Formal));

        b.openai_system_prompt = Some("Be terse.".into());
        b.openai_model = Some(String::new());
        let cfg = binding_openai_config(Some(&b), &OpenAiConfig::default()).unwrap();
        assert_eq!(cfg.system_prompt, "Be terse.");
        assert_eq!(cfg.model, OpenAiConfig::default().model, "an empty model must not override");
    }

    #[test]
    fn a_custom_preset_keeps_the_global_system_prompt() {
        let mut b = binding();
        b.openai_enabled = Some(true);
        b.openai_mode = Some("custom".into());
        let base = OpenAiConfig { system_prompt: "Global.".into(), ..Default::default() };
        assert_eq!(binding_openai_config(Some(&b), &base).unwrap().system_prompt, "Global.");
    }
}
