//! Linux: Wayland first (data-control), X11 as the fallback.
//!
//! Which one is right is not obvious from the environment. A Wayland session
//! also has an X server (XWayland), and a compositor without the data-control
//! protocol — GNOME — rejects the Wayland route but bridges the X11 clipboard
//! to its native clients. So each operation tries the likely backend and falls
//! back to the other rather than trusting `WAYLAND_DISPLAY` alone.

use anyhow::{anyhow, Result};
use tracing::debug;

use crate::{wayland, x11};

#[derive(Clone, Copy, PartialEq, Debug)]
enum Backend {
    Wayland,
    X11,
}

fn order() -> Vec<Backend> {
    let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
    let x11 = std::env::var_os("DISPLAY").is_some();
    let mut v = Vec::new();
    if wayland {
        v.push(Backend::Wayland);
    }
    if x11 {
        v.push(Backend::X11);
    }
    v
}

pub enum Snapshot {
    Wayland(wayland::Snapshot),
    X11(x11::Snapshot),
}

pub enum Held {
    Wayland(wayland::Held),
    X11(x11::Held),
}

pub fn snapshot() -> Result<Snapshot> {
    let mut last = anyhow!("no display server found (neither WAYLAND_DISPLAY nor DISPLAY is set)");
    for b in order() {
        let r = match b {
            Backend::Wayland => wayland::snapshot().map(Snapshot::Wayland),
            Backend::X11 => x11::snapshot().map(Snapshot::X11),
        };
        match r {
            Ok(s) => return Ok(s),
            Err(e) => {
                debug!("clipboard snapshot via {b:?} failed: {e:#}");
                last = e;
            }
        }
    }
    Err(last)
}

pub fn set_text(text: &str) -> Result<Held> {
    let mut last = anyhow!("no display server found (neither WAYLAND_DISPLAY nor DISPLAY is set)");
    for b in order() {
        let r = match b {
            Backend::Wayland => wayland::set_text(text).map(Held::Wayland),
            Backend::X11 => x11::set_text(text).map(Held::X11),
        };
        match r {
            Ok(h) => return Ok(h),
            Err(e) => {
                debug!("clipboard set via {b:?} failed: {e:#}");
                last = e;
            }
        }
    }
    Err(last)
}

pub fn current_text() -> Option<String> {
    for b in order() {
        let t = match b {
            Backend::Wayland => wayland::current_text(),
            Backend::X11 => x11::current_text(),
        };
        if t.is_some() {
            return t;
        }
    }
    None
}

pub fn restore(s: Snapshot) -> Result<()> {
    match s {
        Snapshot::Wayland(s) => wayland::restore(s),
        Snapshot::X11(s) => x11::restore(s),
    }
}

impl Snapshot {
    pub fn format_count(&self) -> usize {
        match self {
            Snapshot::Wayland(s) => s.format_count(),
            Snapshot::X11(s) => s.format_count(),
        }
    }
}

impl Held {
    pub fn still_current(&self) -> bool {
        match self {
            Held::Wayland(h) => h.still_current(),
            Held::X11(h) => h.still_current(),
        }
    }
    pub fn tracks_reads(&self) -> bool {
        matches!(self, Held::X11(_))
    }
    pub fn mark(&self) -> usize {
        match self {
            Held::Wayland(_) => 0,
            Held::X11(h) => h.mark(),
        }
    }
    pub fn read_since(&self, mark: usize) -> bool {
        match self {
            Held::Wayland(_) => false,
            Held::X11(h) => h.read_since(mark),
        }
    }
}
