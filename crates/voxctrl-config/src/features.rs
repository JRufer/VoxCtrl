//! Text post-processing features.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    pub remove_fillers: bool,
    pub custom_vocabulary: Vec<String>,
    pub spoken_punctuation: bool,
    pub auto_format_lists: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub show_notification: Option<bool>,
    /// Map of trigger → expansion, e.g. {"addr" → "123 Main St"}
    pub snippets: std::collections::HashMap<String, String>,
    /// Transcribe the opening seconds of a recording while it is still going,
    /// so a spoken voice command shows its overlay and starts loading the TTS
    /// model before the user releases the hotkey. Costs some extra
    /// transcription work at the start of each recording.
    #[serde(default = "default_early_command_detection")]
    pub early_command_detection: bool,
}

fn default_early_command_detection() -> bool {
    true
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            remove_fillers: true,
            custom_vocabulary: vec!["VoxCtrl".into(), "Hey Vox".into()],
            spoken_punctuation: true,
            auto_format_lists: true,
            show_notification: None,
            snippets: std::collections::HashMap::new(),
            early_command_detection: true,
        }
    }
}
