//! Microphone capture settings.

use serde::{Deserialize, Serialize};

fn default_gain() -> f32 {
    1.0
}

fn default_dynamic_stream() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioConfig {
    pub vad_threshold: f32,
    /// None = use default system device
    pub input_device_index: Option<u32>,
    /// Saved evdev device path, e.g. "/dev/input/event4" (Linux only)
    pub evdev_device: Option<String>,
    pub noise_suppression: bool,
    /// Linear gain multiplier applied before sending to inference (1.0 = unity)
    #[serde(default = "default_gain")]
    pub gain: f32,
    #[serde(default = "default_dynamic_stream")]
    pub dynamic_stream: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            vad_threshold: 0.5,
            input_device_index: None,
            evdev_device: None,
            noise_suppression: false,
            gain: 1.0,
            dynamic_stream: true,
        }
    }
}
