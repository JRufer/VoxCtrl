//! Microphone capture settings.

use serde::{Deserialize, Serialize};

fn default_gain() -> f32 {
    1.0
}

fn default_dynamic_stream() -> bool {
    true
}

fn default_max_recording_secs() -> u32 {
    600
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
    /// The longest one recording may run, in seconds; 0 for no limit.
    ///
    /// A recording is held in memory until it is transcribed, so one that is
    /// never stopped (a toggle shortcut pressed once and forgotten, a key that
    /// stuck) grows until the machine runs out. At the limit the recording
    /// ends as if the shortcut had been released: what was said is
    /// transcribed and delivered, and a notification says why it stopped.
    #[serde(default = "default_max_recording_secs")]
    pub max_recording_secs: u32,
}

impl AudioConfig {
    /// [`max_recording_secs`](Self::max_recording_secs) as a number of 16 kHz
    /// samples, or `None` when there is no limit.
    pub fn max_recording_samples(&self) -> Option<usize> {
        (self.max_recording_secs > 0).then(|| self.max_recording_secs as usize * 16_000)
    }
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
            max_recording_secs: default_max_recording_secs(),
        }
    }
}
