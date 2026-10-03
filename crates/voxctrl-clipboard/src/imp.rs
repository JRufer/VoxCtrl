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
    // The first backend that sees content wins. An *empty* answer is not
    // trusted while another backend is still to be asked: on a Wayland
    // session the X11 clipboard (XWayland) and the Wayland one are bridged
    // lazily, so one can look empty while the other holds the copy.
    let mut empty: Option<Snapshot> = None;
    for b in order() {
        let r = match b {
            Backend::Wayland => wayland::snapshot().map(Snapshot::Wayland),
            Backend::X11 => x11::snapshot().map(Snapshot::X11),
        };
        match r {
            Ok(s) if s.format_count() > 0 => {
                tracing::info!(backend = ?b, formats = s.format_count(), "clipboard backed up");
                return Ok(s);
            }
            Ok(s) => {
                debug!("clipboard snapshot via {b:?} found nothing");
                empty.get_or_insert(s);
            }
            Err(e) => {
                tracing::warn!("clipboard snapshot via {b:?} failed: {e:#}");
                last = e;
            }
        }
    }
    // Only an empty answer is trusted as "empty"; if every backend that
    // answered at all failed, say so rather than claim an empty clipboard.
    empty.ok_or(last)
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
    fn into_wayland(self) -> wayland::Snapshot {
        match self {
            Snapshot::Wayland(s) => s,
            Snapshot::X11(s) => wayland::Snapshot::from_generic(s.into_generic()),
        }
    }

    fn into_x11(self) -> x11::Snapshot {
        match self {
            Snapshot::X11(s) => s,
            Snapshot::Wayland(s) => x11::Snapshot::from_generic(s.into_generic()),
        }
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
    /// Put `s` back through the backend that holds the borrowed text, which is
    /// not necessarily the one the snapshot was taken through (the two can
    /// disagree about which is usable). Restoring through the wrong one would
    /// leave the dictation on the real clipboard.
    pub fn restore(&self, s: Snapshot) -> Result<()> {
        match self {
            Held::Wayland(_) => wayland::restore(s.into_wayland()),
            Held::X11(_) => x11::restore(s.into_x11()),
        }
    }

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
