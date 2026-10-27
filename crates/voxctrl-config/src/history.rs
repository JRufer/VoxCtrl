//! Dictation history.
//!
//! History is private by default: it lives in memory, holds a bounded number
//! of recent transcripts, and is gone when VoxCtrl quits. Keeping it across
//! restarts is a separate opt-in, and handing it to MCP clients another.

use serde::{Deserialize, Serialize};

fn default_enabled() -> bool {
    true
}
fn default_max_entries() -> u32 {
    50
}
fn default_max_age_hours() -> u32 {
    24
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryConfig {
    /// Keep recent transcripts at all. Off: nothing is recorded, and what was
    /// kept is cleared.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Keep history between restarts, encrypted, with the key in the system
    /// keyring. Off by default; turning it off deletes the file and the key.
    #[serde(default)]
    pub persist: bool,
    /// The most transcripts kept; older ones are dropped first.
    #[serde(default = "default_max_entries")]
    pub max_entries: u32,
    /// Transcripts older than this are dropped. 0 keeps them until the
    /// entry limit pushes them out.
    #[serde(default = "default_max_age_hours")]
    pub max_age_hours: u32,
    /// Let MCP clients read recent transcripts (`get_recent_transcripts`).
    /// Off by default: any program that can reach the MCP socket could then
    /// read what you dictated.
    #[serde(default)]
    pub mcp_access: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            persist: false,
            max_entries: default_max_entries(),
            max_age_hours: default_max_age_hours(),
            mcp_access: false,
        }
    }
}

/// The largest `max_entries` the settings accept.
pub const HISTORY_MAX_ENTRIES: u32 = 1000;
