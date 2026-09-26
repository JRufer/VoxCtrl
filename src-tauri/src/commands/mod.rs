//! The Tauri commands the frontend invokes, grouped by area. Each submodule is
//! re-exported whole, so `use crate::commands::*` (and `generate_handler!`)
//! see one flat namespace, exactly as when this was a single file.

mod core;
mod routing;
mod tts;
mod models;
mod overlays;
mod audio;
mod openai;
mod hotkeys;
mod setup;
mod display;

pub use core::*;
pub use routing::*;
pub use tts::*;
pub use models::*;
pub use overlays::*;
pub use audio::*;
pub use openai::*;
pub use hotkeys::*;
pub use setup::*;
pub use display::*;
