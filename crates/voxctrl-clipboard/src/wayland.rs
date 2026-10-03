//! Wayland clipboard through the data-control protocol (wlr-data-control or
//! ext-data-control), which lets a background client read and set the
//! clipboard without owning a focused window.
//!
//! Compositors that do not implement it (GNOME's Mutter) report a missing
//! protocol here; the caller then falls back to the X11 backend, which Mutter
//! bridges to its native clients.

use std::collections::HashSet;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use wl_clipboard_rs::copy::{self, MimeSource, Options, Source};
use wl_clipboard_rs::paste::{self, ClipboardType, Seat};

/// A clipboard larger than this is not worth round-tripping.
const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;
/// How long one format may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_millis(700);

pub struct Snapshot {
    items: Vec<(String, Vec<u8>)>,
}

impl Snapshot {
    pub fn format_count(&self) -> usize {
        self.items.len()
    }

    /// The formats as (mime type, bytes), for restoring through another backend.
    pub fn into_generic(self) -> Vec<(String, Vec<u8>)> {
        self.items
    }

    pub fn from_generic(items: Vec<(String, Vec<u8>)>) -> Self {
        Snapshot { items }
    }
}

pub fn snapshot() -> Result<Snapshot> {
    let types: HashSet<String> = match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
        Ok(t) => t,
        Err(paste::Error::ClipboardEmpty) => return Ok(Snapshot { items: Vec::new() }),
        Err(e) => bail!("{e}"),
    };

    let mut items = Vec::new();
    let mut skipped = Vec::new();
    let mut total = 0usize;
    for mime in &types {
        // One format that cannot be read must not cost the others: a
        // clipboard owner (or the compositor's X11 bridge) can answer for
        // most of what it offers and stall on one type. That one is skipped.
        match read_one(mime) {
            Ok(data) => {
                total += data.len();
                if total > MAX_TOTAL_BYTES {
                    bail!("clipboard too large to back up");
                }
                items.push((mime.clone(), data));
            }
            Err(e) => {
                tracing::warn!("clipboard format {mime} could not be backed up: {e:#}");
                skipped.push(mime.clone());
            }
        }
    }
    if items.is_empty() && !types.is_empty() {
        // The clipboard has content that could not be read. That is not the
        // same as an empty clipboard, and must not be reported as one: a
        // restore of "empty" would clear it.
        bail!("none of the {} clipboard formats could be read", types.len());
    }
    if !skipped.is_empty() {
        tracing::warn!("{} of {} clipboard formats could not be backed up: {skipped:?}", skipped.len(), types.len());
    }
    Ok(Snapshot { items })
}

/// Read one format. The owner writes into a pipe, and one that never finishes
/// would block forever, so the read has a deadline.
fn read_one(mime: &str) -> Result<Vec<u8>> {
    let (mut reader, _) =
        paste::get_contents(ClipboardType::Regular, Seat::Unspecified, paste::MimeType::Specific(mime))
            .map_err(|e| anyhow!("{e}"))?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let r = reader.read_to_end(&mut data).map(|_| data);
        let _ = tx.send(r);
    });
    match rx.recv_timeout(READ_TIMEOUT) {
        Ok(Ok(d)) => Ok(d),
        Ok(Err(e)) => bail!("read failed: {e}"),
        Err(_) => bail!("the clipboard owner did not finish sending it"),
    }
}

/// The clipboard's text, if it holds any.
pub fn current_text() -> Option<String> {
    let (mut reader, _) =
        paste::get_contents(ClipboardType::Regular, Seat::Unspecified, paste::MimeType::Text).ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut data = Vec::new();
        let r = reader.read_to_end(&mut data).map(|_| data);
        let _ = tx.send(r);
    });
    let data = rx.recv_timeout(READ_TIMEOUT).ok()?.ok()?;
    Some(String::from_utf8_lossy(&data).into_owned())
}

pub struct Held {
    owned: Arc<AtomicBool>,
}

impl Held {
    pub fn still_current(&self) -> bool {
        self.owned.load(Ordering::SeqCst)
    }
}

/// Serve `sources` from a background thread until another client takes the
/// clipboard. Returns once the compositor has accepted the selection, or the
/// error that stopped it.
fn serve(sources: Vec<MimeSource>, sensitive: bool, exact: bool) -> Result<Held> {
    let owned = Arc::new(AtomicBool::new(false));
    let owned_t = owned.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();

    std::thread::Builder::new()
        .name("voxctrl-clipboard-owner".into())
        .spawn(move || {
            let mut opts = Options::new();
            // Required by `prepare_copy_multi` (it asserts it): serve from
            // this thread rather than forking a background process.
            opts.foreground(true);
            opts.sensitive(sensitive);
            // wl-clipboard-rs offers extra plain-text types whenever a text
            // type is present. For a restore that would invent formats the
            // user never had — an HTML-only clipboard would gain a
            // `text/plain` holding the markup — so a restore offers exactly
            // what was saved.
            opts.omit_additional_text_mime_types(exact);
            let prepared = match opts.prepare_copy_multi(sources) {
                Ok(p) => p,
                Err(e) => {
                    let _ = tx.send(Err(anyhow!("{e}")));
                    return;
                }
            };
            owned_t.store(true, Ordering::SeqCst);
            let _ = tx.send(Ok(()));
            let _ = prepared.serve();
            owned_t.store(false, Ordering::SeqCst);
        })?;

    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| anyhow!("timed out taking the clipboard"))??;
    Ok(Held { owned })
}

pub fn set_text(text: &str) -> Result<Held> {
    serve(
        vec![MimeSource {
            source: Source::Bytes(text.as_bytes().to_vec().into_boxed_slice()),
            mime_type: copy::MimeType::Text,
        }],
        true,
        false,
    )
}

pub fn restore(s: Snapshot) -> Result<()> {
    if s.items.is_empty() {
        copy::clear(copy::ClipboardType::Regular, copy::Seat::All).map_err(|e| anyhow!("{e}"))?;
        return Ok(());
    }
    let sources = s
        .items
        .into_iter()
        .map(|(mime, data)| MimeSource {
            source: Source::Bytes(data.into_boxed_slice()),
            mime_type: copy::MimeType::Specific(mime),
        })
        .collect();
    serve(sources, false, true).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ENV_LOCK;
    use std::process::{Child, Command, Stdio};

    /// A headless sway, which implements the data-control protocol this
    /// backend uses. Skipped (the tests return early) where sway is missing.
    struct Sway(Child, std::path::PathBuf);

    impl Sway {
        fn start() -> Option<Self> {
            let dir = std::env::temp_dir().join(format!("voxctrl-wl-{}", std::process::id()));
            std::fs::create_dir_all(&dir).ok()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).ok()?;
            }
            let child = Command::new("sway")
                .env("XDG_RUNTIME_DIR", &dir)
                .env("WLR_BACKENDS", "headless")
                .env("WLR_LIBINPUT_NO_DEVICES", "1")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            std::env::set_var("XDG_RUNTIME_DIR", &dir);
            std::env::set_var("WAYLAND_DISPLAY", "wayland-1");
            std::env::remove_var("DISPLAY");
            for _ in 0..100 {
                if dir.join("wayland-1").exists() {
                    std::thread::sleep(Duration::from_millis(300));
                    return Some(Sway(child, dir));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            None
        }
    }

    impl Drop for Sway {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
            let _ = std::fs::remove_dir_all(&self.1);
        }
    }

    fn sorted(s: &Snapshot) -> Vec<(String, Vec<u8>)> {
        let mut v = s.items.clone();
        v.sort();
        v
    }

    #[test]
    fn a_restore_offers_exactly_what_was_saved_and_a_dictation_pastes() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(_s) = Sway::start() else {
            eprintln!("sway not available; skipping");
            return;
        };

        // A browser-style clipboard: HTML and plain text with *different*
        // contents, and an image. Nothing here is offered as UTF8_STRING, and
        // a restore must not invent it (from whichever text format came
        // first, which is how the HTML ended up being pasted as plain text).
        let original = Snapshot {
            items: vec![
                ("text/html".into(), b"<b>rich</b>".to_vec()),
                ("text/plain".into(), b"plain version".to_vec()),
                ("image/png".into(), (0..5000u32).map(|i| (i % 251) as u8).collect()),
            ],
        };
        restore(Snapshot { items: original.items.clone() }).unwrap();
        let saved = snapshot().unwrap();
        assert_eq!(sorted(&saved), sorted(&original), "restore changed the formats offered");

        // Borrow for a dictation.
        let held = set_text("dictated words").unwrap();
        assert!(held.still_current());
        let during = snapshot().unwrap();
        assert!(
            during.items.iter().any(|(m, d)| m.starts_with("text/plain") && d == b"dictated words"),
            "the dictation was not on the clipboard"
        );

        // And back.
        assert!(held.still_current());
        restore(saved).unwrap();
        let after = snapshot().unwrap();
        assert_eq!(sorted(&after), sorted(&original));
    }
}
