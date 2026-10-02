//! End to end on a private X server: a stand-in "application" receives the
//! paste shortcut, asks for the clipboard like a real one would, and the test
//! checks both what it received and that the clipboard was put back.

#![cfg(target_os = "linux")]

use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use x11rb::connection::Connection;
use x11rb::protocol::xproto::*;
use x11rb::protocol::Event;
use x11rb::wrapper::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;
use x11rb::{COPY_DEPTH_FROM_PARENT, CURRENT_TIME};

static LOCK: Mutex<()> = Mutex::new(());

struct Xvfb(Child);

impl Xvfb {
    fn start() -> Option<Self> {
        let display = format!(":{}", 600 + (std::process::id() % 300));
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

fn intern(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes()).unwrap().reply().unwrap().atom
}

/// Ask the clipboard owner for UTF8_STRING, as an application pasting would.
fn read_clipboard(conn: &RustConnection, win: u32) -> Option<String> {
    let (clip, utf8, prop) = (intern(conn, "CLIPBOARD"), intern(conn, "UTF8_STRING"), intern(conn, "TEST_PROP"));
    conn.delete_property(win, prop).unwrap();
    conn.convert_selection(win, clip, utf8, prop, CURRENT_TIME).unwrap();
    conn.flush().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if let Some(Event::SelectionNotify(e)) = conn.poll_for_event().unwrap() {
            if e.property == 0 {
                return None;
            }
            let r = conn.get_property(true, win, prop, AtomEnum::ANY, 0, 1 << 16).unwrap().reply().unwrap();
            return Some(String::from_utf8_lossy(&r.value).into_owned());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    None
}

struct App {
    /// What the application pasted, and the modifier state it saw doing so.
    pasted: Arc<Mutex<Option<(String, u16)>>>,
}

/// A focused window that pastes the clipboard when it sees Ctrl+V / Ctrl+Shift+V.
fn spawn_app(class: &str) -> App {
    let pasted: Arc<Mutex<Option<(String, u16)>>> = Arc::new(Mutex::new(None));
    let out = pasted.clone();
    let class = class.to_string();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (conn, screen) = RustConnection::connect(None).unwrap();
        let root = conn.setup().roots[screen].root;
        let win = conn.generate_id().unwrap();
        conn.create_window(
            COPY_DEPTH_FROM_PARENT, win, root, 0, 0, 32, 32, 0, WindowClass::INPUT_OUTPUT, 0,
            &CreateWindowAux::new().event_mask(EventMask::KEY_PRESS | EventMask::PROPERTY_CHANGE),
        )
        .unwrap();
        let wm_class = format!("{class}\0{class}\0");
        conn.change_property8(PropMode::REPLACE, win, AtomEnum::WM_CLASS, AtomEnum::STRING, wm_class.as_bytes()).unwrap();
        conn.map_window(win).unwrap();
        conn.set_input_focus(InputFocus::POINTER_ROOT, win, CURRENT_TIME).unwrap();
        // The window manager's view of "active window".
        let active = intern(&conn, "_NET_ACTIVE_WINDOW");
        conn.change_property32(PropMode::REPLACE, root, active, AtomEnum::WINDOW, &[win]).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
        ready_tx.send(()).unwrap();

        // Ctrl+V's keycode on a stock evdev keymap is 55.
        while let Ok(ev) = conn.wait_for_event() {
            if let Event::KeyPress(k) = ev {
                if k.detail == 55 && u16::from(k.state) & u16::from(KeyButMask::CONTROL) != 0 {
                    if let Some(text) = read_clipboard(&conn, win) {
                        *out.lock().unwrap() = Some((text, u16::from(k.state)));
                    }
                }
            }
        }
    });
    ready_rx.recv().unwrap();
    App { pasted }
}

fn wait_for(app: &App) -> Option<(String, u16)> {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Some(p) = app.pasted.lock().unwrap().clone() {
            return Some(p);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    None
}

fn clipboard_text_now() -> Option<String> {
    let (conn, screen) = RustConnection::connect(None).unwrap();
    let root = conn.setup().roots[screen].root;
    let win = conn.generate_id().unwrap();
    conn.create_window(COPY_DEPTH_FROM_PARENT, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new()).unwrap();
    read_clipboard(&conn, win)
}

#[tokio::test]
async fn a_dictation_is_pasted_and_the_clipboard_is_put_back() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_x) = Xvfb::start() else {
        eprintln!("Xvfb not available; skipping");
        return;
    };
    voxctrl_inject::set_paste_shortcut("auto");

    // The user has something copied.
    let _user = voxctrl_clipboard::set_text("what the user had copied").unwrap();

    let app = spawn_app("firefox");
    voxctrl_inject::inject_text_with("hello from the dictation", true).await.unwrap();

    let (text, state) = wait_for(&app).expect("the application never pasted");
    assert_eq!(text, "hello from the dictation");
    assert_eq!(state & u16::from(KeyButMask::SHIFT), 0, "an ordinary window gets Ctrl+V, not Ctrl+Shift+V");

    assert_eq!(clipboard_text_now().as_deref(), Some("what the user had copied"));
}

#[tokio::test]
async fn a_terminal_gets_the_terminal_shortcut() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_x) = Xvfb::start() else {
        eprintln!("Xvfb not available; skipping");
        return;
    };
    voxctrl_inject::set_paste_shortcut("auto");
    let app = spawn_app("gnome-terminal-server");
    voxctrl_inject::inject_text_with("echo hi", true).await.unwrap();

    let (text, state) = wait_for(&app).expect("the terminal never pasted");
    assert_eq!(text, "echo hi");
    assert_ne!(state & u16::from(KeyButMask::SHIFT), 0, "a terminal gets Ctrl+Shift+V");
    // Nothing was on the clipboard before, so nothing is afterwards.
    assert_eq!(clipboard_text_now(), None);
}

#[tokio::test]
async fn repeated_dictations_keep_pasting() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_x) = Xvfb::start() else {
        eprintln!("Xvfb not available; skipping");
        return;
    };
    voxctrl_inject::set_paste_shortcut("auto");
    let _user = voxctrl_clipboard::set_text("what the user had copied").unwrap();
    let app = spawn_app("firefox");
    for i in 0..6 {
        *app.pasted.lock().unwrap() = None;
        let t0 = Instant::now();
        voxctrl_inject::inject_text_with(&format!("dictation number {i}"), true).await.unwrap();
        let (text, _) = wait_for(&app).unwrap_or_else(|| panic!("dictation {i} never pasted"));
        assert_eq!(text, format!("dictation number {i}"));
        eprintln!("dictation {i}: {:?}", t0.elapsed());
        assert_eq!(clipboard_text_now().as_deref(), Some("what the user had copied"), "after {i}");
    }
}

/// An application that owns the clipboard and never answers requests for it
/// (frozen, or stuck rendering a large item).
fn spawn_unresponsive_owner() -> std::sync::mpsc::Sender<()> {
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (conn, screen) = RustConnection::connect(None).unwrap();
        let root = conn.setup().roots[screen].root;
        let win = conn.generate_id().unwrap();
        conn.create_window(COPY_DEPTH_FROM_PARENT, win, root, 0, 0, 1, 1, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new()).unwrap();
        let clip = intern(&conn, "CLIPBOARD");
        conn.set_selection_owner(win, clip, CURRENT_TIME).unwrap();
        conn.get_input_focus().unwrap().reply().unwrap();
        ready_tx.send(()).unwrap();
        let _ = stop_rx.recv(); // never reads an event
    });
    ready_rx.recv().unwrap();
    stop_tx
}

#[tokio::test]
async fn a_frozen_clipboard_owner_does_not_stop_dictation() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_x) = Xvfb::start() else {
        eprintln!("Xvfb not available; skipping");
        return;
    };
    voxctrl_inject::set_paste_shortcut("auto");
    let _frozen = spawn_unresponsive_owner();
    let app = spawn_app("firefox");

    let t0 = Instant::now();
    voxctrl_inject::inject_text_with("still gets through", true).await.unwrap();
    let (text, _) = wait_for(&app).expect("the dictation never pasted");
    assert_eq!(text, "still gets through");
    eprintln!("took {:?}", t0.elapsed());
    assert!(t0.elapsed() < Duration::from_secs(8));
}

#[tokio::test]
async fn two_dictations_at_once_do_not_corrupt_each_other() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(_x) = Xvfb::start() else {
        eprintln!("Xvfb not available; skipping");
        return;
    };
    voxctrl_inject::set_paste_shortcut("auto");
    let _user = voxctrl_clipboard::set_text("what the user had copied").unwrap();
    let app = spawn_app("firefox");
    let (a, b) = tokio::join!(
        voxctrl_inject::inject_text_with("first", true),
        voxctrl_inject::inject_text_with("second", true)
    );
    a.unwrap();
    b.unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(clipboard_text_now().as_deref(), Some("what the user had copied"));
    drop(app);
}
