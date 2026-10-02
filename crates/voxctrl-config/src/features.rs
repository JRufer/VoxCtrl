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
    /// Deliver dictated text to the focused app as a single paste (Ctrl+V)
    /// rather than typing it key by key. The previous clipboard contents are
    /// put back afterwards. Falls back to typing if a paste cannot be sent.
    #[serde(default = "default_paste_instead_of_typing")]
    pub paste_instead_of_typing: bool,
    /// The paste shortcut: `"auto"` (Ctrl+V, or what the focused window needs:
    /// Ctrl+Shift+V in terminals, Shift+Insert in Windows consoles),
    /// `"ctrl+v"`, `"ctrl+shift+v"` or `"shift+insert"`. Forcing one is for
    /// windows auto-detection cannot see into, such as a native Wayland
    /// terminal on GNOME.
    #[serde(default = "default_paste_shortcut")]
    pub paste_shortcut: String,
}

fn default_paste_shortcut() -> String {
    "auto".into()
}

fn default_paste_instead_of_typing() -> bool {
    true
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
            paste_instead_of_typing: true,
            paste_shortcut: "auto".into(),
        }
    }
}
