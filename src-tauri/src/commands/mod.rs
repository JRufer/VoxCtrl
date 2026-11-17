//! The Tauri commands the frontend invokes, grouped by area. Each submodule is
//! re-exported whole, so `use crate::commands::*` (and `generate_handler!`)
//! see one flat namespace, exactly as when this was a single file.

mod audio;
mod benchmark;
mod core;
mod display;
mod history;
mod hotkeys;
mod models;
mod openai;
mod overlays;
mod routing;
mod setup;
mod tts;

pub use audio::*;
pub use benchmark::*;
pub use core::*;
pub use display::*;
pub use history::*;
pub use hotkeys::*;
pub use models::*;
pub use openai::*;
pub use overlays::*;
pub use routing::*;
pub use setup::*;
pub use tts::*;
