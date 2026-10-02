//! Full-fidelity clipboard backup and restore.
//!
//! Pasting a dictation means borrowing the clipboard for a moment. Taking the
//! user's clipboard permanently as a side effect of dictating is its own bug,
//! and "restore the text" is not enough: someone with an image, a file list or
//! formatted text on the clipboard would lose it. So [`snapshot`] records
//! *every* format the clipboard offers, and [`restore`] puts them all back.
//!
//! Typical use:
//!
//! ```ignore
//! let saved = snapshot()?;
//! let held = set_text("dictated words")?;
//! // ... send the paste shortcut ...
//! if held.still_current() { restore(saved)?; }
//! ```
//!
//! `still_current` matters: if the user copied something else in the meantime,
//! restoring would clobber that, so the restore is skipped.
//!
//! * **Windows** — every `HGLOBAL` format (text, HTML, RTF, DIB images, file
//!   lists, registered and private formats). GDI-handle formats are skipped;
//!   Windows synthesises them from the memory formats.
//! * **X11** — every target the owner offers, including large ones sent in
//!   chunks (`INCR`); this process then serves them again as the owner.
//! * **Wayland** — every MIME type, via the data-control protocol. Compositors
//!   without it (GNOME) go through XWayland instead.

use std::time::Duration;

use anyhow::Result;

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(target_os = "linux")]
mod x11;
#[cfg(target_os = "linux")]
mod imp;

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod imp;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
mod imp {
    use super::*;
    pub struct Snapshot;
    pub struct Held;
    pub fn snapshot() -> Result<Snapshot> { anyhow::bail!("clipboard not supported on this platform") }
    pub fn set_text(_: &str) -> Result<Held> { anyhow::bail!("clipboard not supported on this platform") }
    pub fn restore(_: Snapshot) -> Result<()> { Ok(()) }
    impl Snapshot { pub fn format_count(&self) -> usize { 0 } }
    impl Held {
        pub fn still_current(&self) -> bool { false }
        pub fn mark(&self) -> usize { 0 }
        pub fn tracks_reads(&self) -> bool { false }
        pub fn read_since(&self, _: usize) -> bool { false }
    }
}

/// Everything the clipboard held at one moment.
pub struct Snapshot(imp::Snapshot);

/// Text this crate placed on the clipboard. Tells the caller whether it is
/// still there, and (where the platform allows) whether anything has read it.
pub struct Held(imp::Held);

/// Record every format the clipboard currently offers.
///
/// An empty clipboard is a valid snapshot: restoring it clears the clipboard.
pub fn snapshot() -> Result<Snapshot> {
    imp::snapshot().map(Snapshot)
}

/// Put `text` on the clipboard, asking clipboard managers and cloud history
/// not to record it (dictation can be sensitive).
pub fn set_text(text: &str) -> Result<Held> {
    imp::set_text(text).map(Held)
}

/// Put a snapshot back, replacing whatever is on the clipboard now. Callers
/// should check [`Held::still_current`] first.
pub fn restore(snapshot: Snapshot) -> Result<()> {
    imp::restore(snapshot.0)
}

impl Snapshot {
    /// How many formats were captured (0 = the clipboard was empty).
    pub fn format_count(&self) -> usize {
        self.0.format_count()
    }
}

impl Held {
    /// Whether the text we placed is still what the clipboard holds. `false`
    /// once anything else has taken the clipboard over.
    pub fn still_current(&self) -> bool {
        self.0.still_current()
    }

    /// Whether this backend can tell when an application reads the text
    /// (see [`Held::read_since`]). When it cannot, callers wait a fixed time.
    pub fn tracks_reads(&self) -> bool {
        self.0.tracks_reads()
    }

    /// A marker to pass to [`Held::read_since`] later.
    pub fn mark(&self) -> usize {
        self.0.mark()
    }

    /// Whether an application has read the text since `mark`. Only meaningful
    /// on X11, where this process serves the clipboard and sees the requests;
    /// everywhere else it is always `false`, and callers fall back to a delay.
    pub fn read_since(&self, mark: usize) -> bool {
        self.0.read_since(mark)
    }

    /// Wait up to `timeout` for [`Held::read_since`] to become true.
    pub fn wait_read_since(&self, mark: usize, timeout: Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        while std::time::Instant::now() < deadline {
            if self.read_since(mark) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.read_since(mark)
    }
}
