//! The MCP server.

use serde::{Deserialize, Serialize};

fn default_visual_feedback() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub server_enabled: bool,
    pub record_timeout: f64,
    #[serde(default = "default_visual_feedback")]
    pub visual_feedback: bool,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            server_enabled: false,
            record_timeout: 15.0,
            visual_feedback: true,
        }
    }
}
