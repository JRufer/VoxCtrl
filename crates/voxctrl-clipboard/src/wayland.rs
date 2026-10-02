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
use tracing::debug;
use wl_clipboard_rs::copy::{self, MimeSource, Options, Source};
use wl_clipboard_rs::paste::{self, ClipboardType, Seat};

/// A clipboard larger than this is not worth round-tripping.
const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;
/// How long one format may take to arrive.
const READ_TIMEOUT: Duration = Duration::from_millis(1500);

pub struct Snapshot {
    items: Vec<(String, Vec<u8>)>,
}

impl Snapshot {
    pub fn format_count(&self) -> usize {
        self.items.len()
    }
}

pub fn snapshot() -> Result<Snapshot> {
    let types: HashSet<String> = match paste::get_mime_types(ClipboardType::Regular, Seat::Unspecified) {
        Ok(t) => t,
        Err(paste::Error::ClipboardEmpty) => return Ok(Snapshot { items: Vec::new() }),
        Err(e) => bail!("{e}"),
    };

    let mut items = Vec::new();
    let mut total = 0usize;
    for mime in types {
        match paste::get_contents(ClipboardType::Regular, Seat::Unspecified, paste::MimeType::Specific(&mime)) {
            Ok((mut reader, _)) => {
                // The owner writes into a pipe; one that never finishes would
                // block this forever, so each read gets a deadline.
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let mut data = Vec::new();
                    let r = reader.read_to_end(&mut data).map(|_| data);
                    let _ = tx.send(r);
                });
                let data = match rx.recv_timeout(READ_TIMEOUT) {
                    Ok(Ok(d)) => d,
                    Ok(Err(e)) => bail!("reading {mime} failed: {e}"),
                    Err(_) => bail!("the clipboard owner did not finish sending {mime}"),
                };
                total += data.len();
                if total > MAX_TOTAL_BYTES {
                    bail!("clipboard too large to back up");
                }
                items.push((mime, data));
            }
            Err(e) => debug!("clipboard type {mime} not read: {e}"),
        }
    }
    Ok(Snapshot { items })
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
fn serve(sources: Vec<MimeSource>, sensitive: bool) -> Result<Held> {
    let owned = Arc::new(AtomicBool::new(false));
    let owned_t = owned.clone();
    let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();

    std::thread::Builder::new()
        .name("voxctrl-clipboard-owner".into())
        .spawn(move || {
            let mut opts = Options::new();
            opts.sensitive(sensitive);
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
    serve(sources, false).map(|_| ())
}
