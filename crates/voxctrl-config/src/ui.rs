//! Window, overlay and notification settings.

use serde::{Deserialize, Serialize};

fn default_auto_show_settings() -> bool {
    false
}

fn default_setup_completed() -> bool {
    true
}

fn default_show_notification() -> bool {
    false
}

fn default_overlay_position() -> String {
    "center".into()
}

fn default_overlay_monitor() -> String {
    "primary".into()
}

fn default_show_command_overlay() -> bool {
    true
}

fn default_command_overlay_duration_secs() -> u32 {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub show_overlay: bool,
    pub overlay_style: String,
    #[serde(default = "default_overlay_position")]
    pub overlay_position: String,
    #[serde(default = "default_overlay_monitor")]
    pub overlay_monitor: String,
    #[serde(default = "default_auto_show_settings")]
    pub auto_show_settings: bool,
    #[serde(default = "default_show_notification")]
    pub show_notification: bool,
    #[serde(default = "default_show_command_overlay")]
    pub show_command_overlay: bool,
    #[serde(default = "default_command_overlay_duration_secs")]
    pub command_overlay_duration_secs: u32,
    /// Whether the first-run setup wizard has been finished.
    ///
    /// The serde default is `true` on purpose: a config file written by an
    /// earlier VoxCtrl has no such field, and its owner has plainly already
    /// set the app up by hand. Only `UiConfig::default()` — reached when no
    /// config file exists at all — starts this at `false`, so the wizard runs
    /// exactly once, on a genuinely new machine.
    #[serde(default = "default_setup_completed")]
    pub setup_completed: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            show_overlay: true,
            overlay_style: "mono_bars".into(),
            overlay_position: "center".into(),
            overlay_monitor: "primary".into(),
            auto_show_settings: false,
            show_notification: false,
            show_command_overlay: true,
            command_overlay_duration_secs: 3,
            setup_completed: false,
        }
    }
}
