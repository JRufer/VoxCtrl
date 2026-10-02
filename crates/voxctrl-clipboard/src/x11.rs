//! X11 clipboard: read every target the owner offers, and serve a set of
//! targets as the owner.
//!
//! X11 has no clipboard store: the selection owner *is* the clipboard, and
//! other clients ask it for each format on demand. So a backup means asking
//! the current owner for each target in turn, and a restore means becoming the
//! owner again and answering requests from the saved bytes. This process stays
//! the owner (on a background thread) until something else takes the
//! selection.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Result};
use tracing::{debug, warn};
use x11rb::connection::{Connection, RequestConnection};
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, CreateWindowAux, EventMask, PropMode, SelectionNotifyEvent,
    Window, WindowClass, SELECTION_NOTIFY_EVENT,
};
use x11rb::protocol::Event;
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME, NONE};

/// Per-target wait for the owner to answer.
const TARGET_TIMEOUT: Duration = Duration::from_millis(1500);
/// A clipboard larger than this is not worth round-tripping.
const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;

/// Hint for KDE's Klipper (and others honouring it) not to record the text.
const KDE_EXCLUSION_MIME: &str = "x-kde-passwordManagerHint";
const KDE_EXCLUSION_HINT: &[u8] = b"secret";

/// Targets that describe the selection rather than being content.
const META_TARGETS: &[&str] = &[
    "TARGETS", "MULTIPLE", "TIMESTAMP", "DELETE", "INSERT_PROPERTY", "INSERT_SELECTION",
    "SAVE_TARGETS",
];

#[derive(Clone)]
struct Item {
    /// The target it is requested under.
    target: String,
    /// The property type it is delivered as (often the same as `target`).
    ty: String,
    format: u8,
    data: Vec<u8>,
}

pub struct Snapshot {
    items: Vec<Item>,
}

impl Snapshot {
    pub fn format_count(&self) -> usize {
        self.items.len()
    }
}

struct Atoms {
    clipboard: Atom,
    targets: Atom,
    incr: Atom,
    prop: Atom,
    atom_type: Atom,
}

fn atom(conn: &RustConnection, name: &str) -> Result<Atom> {
    Ok(conn.intern_atom(false, name.as_bytes())?.reply()?.atom)
}

fn atom_name(conn: &RustConnection, a: Atom) -> Result<String> {
    let r = conn.get_atom_name(a)?.reply()?;
    Ok(String::from_utf8_lossy(&r.name).into_owned())
}

fn connect() -> Result<(RustConnection, Window, Atoms)> {
    let (conn, screen) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen].root;
    let win = conn.generate_id()?;
    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        win,
        root,
        0,
        0,
        1,
        1,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
    )?;
    let atoms = Atoms {
        clipboard: atom(&conn, "CLIPBOARD")?,
        targets: atom(&conn, "TARGETS")?,
        incr: atom(&conn, "INCR")?,
        prop: atom(&conn, "VOXCTRL_CLIPBOARD")?,
        atom_type: AtomEnum::ATOM.into(),
    };
    conn.flush()?;
    Ok((conn, win, atoms))
}

// ── Reading ───────────────────────────────────────────────────────────────────

/// Read the whole property, following `bytes_after`.
fn read_property(conn: &RustConnection, win: Window, prop: Atom, delete: bool) -> Result<(Atom, u8, Vec<u8>)> {
    let mut data = Vec::new();
    let mut ty = 0;
    let mut format = 8;
    let mut offset = 0u32;
    loop {
        // `long_length` is in 4-byte units.
        let r = conn.get_property(delete, win, prop, AtomEnum::ANY, offset, 1 << 20)?.reply()?;
        ty = if r.type_ != 0 { r.type_ } else { ty };
        format = if r.format != 0 { r.format } else { format };
        offset += (r.value.len() as u32).div_ceil(4);
        data.extend_from_slice(&r.value);
        if r.bytes_after == 0 || r.value.is_empty() {
            break;
        }
        if data.len() > MAX_TOTAL_BYTES {
            bail!("clipboard property too large");
        }
    }
    Ok((ty, format, data))
}

/// Ask the clipboard owner for `target`, waiting for its answer. `None` when
/// the owner refuses or does not answer in time.
fn fetch(conn: &RustConnection, win: Window, a: &Atoms, target: Atom) -> Result<Option<(Atom, u8, Vec<u8>)>> {
    conn.delete_property(win, a.prop)?;
    conn.convert_selection(win, a.clipboard, target, a.prop, CURRENT_TIME)?;
    conn.flush()?;
    let deadline = Instant::now() + TARGET_TIMEOUT;

    loop {
        match conn.poll_for_event()? {
            Some(Event::SelectionNotify(e)) if e.requestor == win && e.target == target => {
                if e.property == NONE {
                    return Ok(None);
                }
                let (ty, format, data) = read_property(conn, win, a.prop, true)?;
                if ty != a.incr {
                    return Ok(Some((ty, format, data)));
                }
                return read_incr(conn, win, a, deadline).map(Some);
            }
            Some(_) => {}
            None => {
                if Instant::now() > deadline {
                    return Ok(None);
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

/// Large contents arrive as a series of property updates (the INCR protocol).
fn read_incr(conn: &RustConnection, win: Window, a: &Atoms, deadline: Instant) -> Result<(Atom, u8, Vec<u8>)> {
    // The deleting read above told the owner to start sending.
    let mut out = Vec::new();
    let mut ty = 0;
    let mut format = 8;
    // Each chunk gets a fresh window to arrive in; the overall cap is size.
    let mut chunk_deadline = deadline;
    loop {
        match conn.poll_for_event()? {
            Some(Event::PropertyNotify(e))
                if e.window == win
                    && e.atom == a.prop
                    && e.state == x11rb::protocol::xproto::Property::NEW_VALUE =>
            {
                let (t, f, chunk) = read_property(conn, win, a.prop, true)?;
                if chunk.is_empty() {
                    return Ok((ty, format, out));
                }
                ty = t;
                format = f;
                out.extend_from_slice(&chunk);
                if out.len() > MAX_TOTAL_BYTES {
                    bail!("clipboard too large");
                }
                chunk_deadline = Instant::now() + TARGET_TIMEOUT;
            }
            Some(_) => {}
            None => {
                if Instant::now() > chunk_deadline {
                    bail!("timed out receiving a large clipboard item");
                }
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }
}

pub fn snapshot() -> Result<Snapshot> {
    let (conn, win, a) = connect()?;
    let result = (|| -> Result<Snapshot> {
        // No owner: the clipboard is empty.
        if conn.get_selection_owner(a.clipboard)?.reply()?.owner == NONE {
            return Ok(Snapshot { items: Vec::new() });
        }

        let Some((_, _, raw)) = fetch(&conn, win, &a, a.targets)? else {
            // The owner will not even list its formats. Nothing to save, and
            // saying so beats pretending the clipboard was empty.
            bail!("the clipboard owner did not list its formats");
        };
        let targets: Vec<Atom> = raw
            .chunks_exact(4)
            .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        let mut items = Vec::new();
        let mut total = 0usize;
        for t in targets {
            let name = atom_name(&conn, t)?;
            if META_TARGETS.contains(&name.as_str()) {
                continue;
            }
            match fetch(&conn, win, &a, t)? {
                Some((ty, format, data)) if ty != a.atom_type => {
                    total += data.len();
                    if total > MAX_TOTAL_BYTES {
                        bail!("clipboard too large to back up");
                    }
                    items.push(Item { target: name, ty: atom_name(&conn, ty)?, format, data });
                }
                Some(_) => {}
                None => debug!("clipboard target {name} not delivered; skipped"),
            }
        }
        Ok(Snapshot { items })
    })();
    let _ = conn.destroy_window(win);
    let _ = conn.flush();
    result
}

// ── Serving ───────────────────────────────────────────────────────────────────

pub struct Held {
    owned: Arc<AtomicBool>,
    served: Arc<AtomicUsize>,
}

impl Held {
    pub fn still_current(&self) -> bool {
        self.owned.load(Ordering::SeqCst)
    }
    pub fn mark(&self) -> usize {
        self.served.load(Ordering::SeqCst)
    }
    pub fn read_since(&self, mark: usize) -> bool {
        self.served.load(Ordering::SeqCst) > mark
    }
}

fn text_items(text: &str) -> Vec<Item> {
    let utf8 = text.as_bytes().to_vec();
    // STRING is Latin-1. Anything outside it is replaced rather than failing:
    // a client that asks for STRING cannot show it anyway.
    let latin1: Vec<u8> = text.chars().map(|c| if (c as u32) <= 0xFF { c as u32 as u8 } else { b'?' }).collect();
    vec![
        Item { target: "UTF8_STRING".into(), ty: "UTF8_STRING".into(), format: 8, data: utf8.clone() },
        Item { target: "text/plain;charset=utf-8".into(), ty: "text/plain;charset=utf-8".into(), format: 8, data: utf8.clone() },
        Item { target: "text/plain".into(), ty: "text/plain".into(), format: 8, data: utf8 },
        Item { target: "STRING".into(), ty: "STRING".into(), format: 8, data: latin1 },
        Item { target: KDE_EXCLUSION_MIME.into(), ty: KDE_EXCLUSION_MIME.into(), format: 8, data: KDE_EXCLUSION_HINT.to_vec() },
    ]
}

/// Become the clipboard owner and answer requests until something else takes
/// the selection. Returns once ownership is confirmed.
fn serve(items: Vec<Item>) -> Result<Held> {
    let owned = Arc::new(AtomicBool::new(false));
    let served = Arc::new(AtomicUsize::new(0));
    let (tx, rx) = std::sync::mpsc::channel::<Result<()>>();

    let (owned_t, served_t) = (owned.clone(), served.clone());
    std::thread::Builder::new()
        .name("voxctrl-clipboard-owner".into())
        .spawn(move || {
            let setup = (|| -> Result<(RustConnection, Window, Atoms, Vec<(Atom, Atom, u8, Vec<u8>)>)> {
                let (conn, win, a) = connect()?;
                let mut table = Vec::new();
                for it in &items {
                    table.push((atom(&conn, &it.target)?, atom(&conn, &it.ty)?, it.format, it.data.clone()));
                }
                conn.set_selection_owner(win, a.clipboard, CURRENT_TIME)?;
                conn.flush()?;
                if conn.get_selection_owner(a.clipboard)?.reply()?.owner != win {
                    bail!("could not become the clipboard owner");
                }
                Ok((conn, win, a, table))
            })();
            let (conn, win, a, table) = match setup {
                Ok(v) => v,
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            owned_t.store(true, Ordering::SeqCst);
            let _ = tx.send(Ok(()));

            let max_bytes = conn.maximum_request_bytes().saturating_sub(64);
            while let Ok(ev) = conn.wait_for_event() {
                match ev {
                    Event::SelectionClear(e) if e.selection == a.clipboard => break,
                    Event::SelectionRequest(req) if req.selection == a.clipboard => {
                        let property = if req.property == NONE { req.target } else { req.property };
                        let delivered = answer(&conn, &a, &table, max_bytes, req.requestor, property, req.target);
                        if delivered == Answer::Data {
                            served_t.fetch_add(1, Ordering::SeqCst);
                        }
                        let notify = SelectionNotifyEvent {
                            response_type: SELECTION_NOTIFY_EVENT,
                            sequence: 0,
                            time: req.time,
                            requestor: req.requestor,
                            selection: req.selection,
                            target: req.target,
                            property: if delivered == Answer::Refused { NONE } else { property },
                        };
                        let _ = conn.send_event(false, req.requestor, EventMask::NO_EVENT, notify);
                        let _ = conn.flush();
                    }
                    _ => {}
                }
            }
            owned_t.store(false, Ordering::SeqCst);
            let _ = conn.destroy_window(win);
            let _ = conn.flush();
        })?;

    rx.recv_timeout(Duration::from_secs(3))
        .map_err(|_| anyhow!("timed out taking the clipboard"))??;
    Ok(Held { owned, served })
}

#[derive(PartialEq)]
enum Answer {
    Data,
    Meta,
    Refused,
}

fn answer(
    conn: &RustConnection,
    a: &Atoms,
    table: &[(Atom, Atom, u8, Vec<u8>)],
    max_bytes: usize,
    requestor: Window,
    property: Atom,
    target: Atom,
) -> Answer {
    if target == a.targets {
        let mut list: Vec<u32> = vec![a.targets];
        list.extend(table.iter().map(|(t, ..)| *t));
        let bytes: Vec<u8> = list.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let ok = conn
            .change_property(PropMode::REPLACE, requestor, property, a.atom_type, 32, list.len() as u32, &bytes)
            .is_ok();
        return if ok { Answer::Meta } else { Answer::Refused };
    }
    let Some((_, ty, format, data)) = table.iter().find(|(t, ..)| *t == target) else {
        return Answer::Refused;
    };
    if data.len() > max_bytes {
        // Serving this would need INCR. Refusing one huge item is better than
        // failing the whole paste.
        warn!("clipboard item of {} bytes is too large to serve; refusing", data.len());
        return Answer::Refused;
    }
    let units = data.len() / (*format as usize / 8).max(1);
    match conn.change_property(PropMode::REPLACE, requestor, property, *ty, *format, units as u32, data) {
        Ok(_) => Answer::Data,
        Err(_) => Answer::Refused,
    }
}

pub fn set_text(text: &str) -> Result<Held> {
    serve(text_items(text))
}

pub fn restore(s: Snapshot) -> Result<()> {
    if s.items.is_empty() {
        // The clipboard was empty: release ownership so it is empty again.
        let (conn, win, a) = connect()?;
        conn.set_selection_owner(NONE, a.clipboard, CURRENT_TIME)?;
        // A round trip, so the server has applied it before this returns:
        // another connection could otherwise still see the old owner.
        conn.get_input_focus()?.reply()?;
        let _ = conn.destroy_window(win);
        let _ = conn.flush();
        return Ok(());
    }
    serve(s.items).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command, Stdio};

    use crate::ENV_LOCK as LOCK;

    /// A private X server, so the tests neither need a desktop nor touch one.
    struct Xvfb(Child);

    impl Xvfb {
        fn start() -> Option<Self> {
            // A fresh display number each time: the previous test's server may
            // still be shutting down on the old one.
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = NEXT.fetch_add(1, Ordering::SeqCst);
            let display = format!(":{}", 100 + (std::process::id() % 400) * 4 + n);
            let child = Command::new("Xvfb")
                .args([&display, "-screen", "0", "64x64x24", "-nolisten", "tcp", "-noreset"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .ok()?;
            std::env::set_var("DISPLAY", &display);
            std::env::remove_var("WAYLAND_DISPLAY");
            for _ in 0..100 {
                if RustConnection::connect(None).is_ok() {
                    return Some(Xvfb(child));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            None
        }
    }

    impl Drop for Xvfb {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    fn item(target: &str, data: Vec<u8>) -> Item {
        Item { target: target.into(), ty: target.into(), format: 8, data }
    }

    fn sorted(s: &Snapshot) -> Vec<(String, Vec<u8>)> {
        let mut v: Vec<_> = s.items.iter().map(|i| (i.target.clone(), i.data.clone())).collect();
        v.sort();
        v
    }

    #[test]
    fn every_format_survives_a_borrow_and_restore() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(_x) = Xvfb::start() else {
            eprintln!("Xvfb not available; skipping");
            return;
        };

        // Something the user "copied": text, HTML, a file list and a large
        // image-like blob, as separate formats.
        let big: Vec<u8> = (0..2_000_000u32).map(|i| (i % 251) as u8).collect();
        let original = vec![
            item("UTF8_STRING", "my copied text".as_bytes().to_vec()),
            item("text/html", b"<b>my copied text</b>".to_vec()),
            item("text/uri-list", b"file:///tmp/a.txt\r\nfile:///tmp/b.txt\r\n".to_vec()),
            item("image/png", big),
        ];
        let _owner = serve(original.clone()).unwrap();

        let saved = snapshot().unwrap();
        assert_eq!(saved.format_count(), original.len());

        // Borrow the clipboard for a dictation.
        let held = set_text("dictated words").unwrap();
        assert!(held.still_current());
        let mark = held.mark();
        let during = snapshot().unwrap();
        assert!(sorted(&during).iter().any(|(t, d)| t == "UTF8_STRING" && d == b"dictated words"));
        // The snapshot above read the text, so it counts as a read.
        assert!(held.read_since(mark));

        // Restore: every format is back, byte for byte.
        assert!(held.still_current());
        restore(saved).unwrap();
        let after = snapshot().unwrap();
        let mut want: Vec<_> = original.iter().map(|i| (i.target.clone(), i.data.clone())).collect();
        want.sort();
        assert_eq!(sorted(&after), want);
        assert!(!held.still_current(), "the borrowed text was replaced");
    }

    #[test]
    fn an_empty_clipboard_is_restored_to_empty() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(_x) = Xvfb::start() else {
            eprintln!("Xvfb not available; skipping");
            return;
        };
        let saved = snapshot().unwrap();
        assert_eq!(saved.format_count(), 0);
        let held = set_text("dictated words").unwrap();
        assert!(held.still_current());
        restore(saved).unwrap();
        assert_eq!(snapshot().unwrap().format_count(), 0);
    }

    #[test]
    fn a_clipboard_someone_else_took_is_noticed() {
        let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(_x) = Xvfb::start() else {
            eprintln!("Xvfb not available; skipping");
            return;
        };
        let held = set_text("dictated words").unwrap();
        let _user_copy = serve(vec![item("UTF8_STRING", b"user copied this meanwhile".to_vec())]).unwrap();
        // Ownership change is delivered asynchronously.
        for _ in 0..100 {
            if !held.still_current() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!held.still_current());
    }
}
