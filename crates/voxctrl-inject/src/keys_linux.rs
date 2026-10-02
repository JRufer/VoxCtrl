//! Sending the paste shortcut on Linux, by every route available.
//!
//! There is no single way to press a key on Linux: X11 has the XTEST
//! extension, wlroots/KDE compositors accept `wtype`, and `ydotool` writes to
//! `/dev/uinput` and so works anywhere it has permission. They are tried in
//! the order most likely to reach the focused window, and the shortcut itself
//! is chosen for the focused application (terminals paste with Ctrl+Shift+V).

use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use tracing::{debug, warn};
use voxctrl_winput::Shortcut;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    AtomEnum, ConnectionExt as _, Window, KEY_PRESS_EVENT, KEY_RELEASE_EVENT,
};
use x11rb::protocol::xtest::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

// Keysyms.
const XK_V_LOWER: u32 = 0x76;
const XK_V_UPPER: u32 = 0x56;
const XK_SHIFT_L: u32 = 0xffe1;
const XK_CONTROL_L: u32 = 0xffe3;
const XK_INSERT: u32 = 0xff63;

// Linux evdev keycodes + 8, used when the keymap cannot be read.
const FALLBACK_CTRL: u8 = 37;
const FALLBACK_SHIFT: u8 = 50;
const FALLBACK_V: u8 = 55;
const FALLBACK_INSERT: u8 = 118;

/// Window classes of terminal emulators, whose paste shortcut is
/// Ctrl+Shift+V (plain Ctrl+V is a control character there).
const TERMINAL_CLASSES: &[&str] = &[
    "gnome-terminal", "konsole", "xterm", "alacritty", "kitty", "tilix", "terminator",
    "xfce4-terminal", "urxvt", "rxvt", "wezterm", "foot", "ptyxis", "mate-terminal",
    "lxterminal", "termite", "guake", "yakuake", "ghostty", "terminology", "qterminal",
    "sakura", "cool-retro-term", "hyper", "tabby", "contour", "rio", "blackbox",
    "kgx", "console", "deepin-terminal", "st-256color",
];

pub fn is_terminal_class(class: &str) -> bool {
    let c = class.trim().to_ascii_lowercase();
    c == "st" || c.contains("term") || TERMINAL_CLASSES.iter().any(|t| c.contains(t))
}

fn wayland_session() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

fn have(bin: &str) -> bool {
    voxctrl_config::find_in_path(bin).is_some()
}

async fn run(bin: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(bin)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn run_capture(bin: &str, args: &[&str]) -> Option<String> {
    // A compositor query that hangs must not hold up the paste.
    let out = tokio::time::timeout(
        Duration::from_millis(800),
        tokio::process::Command::new(bin)
            .args(args)
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .ok()?
    .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

// ── Choosing the shortcut ─────────────────────────────────────────────────────

/// The class of the focused window, from whatever the session can tell us.
pub async fn focused_window_class() -> Option<String> {
    if wayland_session() {
        // A Wayland compositor does not tell clients which window is focused;
        // some offer a private way to ask. XWayland's idea of "active window"
        // is not trusted here: it keeps pointing at the last X11 window while
        // a native Wayland one has focus.
        if have("hyprctl") {
            if let Some(out) = run_capture("hyprctl", &["activewindow", "-j"]).await {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
                    if let Some(c) = v.get("class").and_then(|c| c.as_str()) {
                        return Some(c.to_string());
                    }
                }
            }
        }
        if have("swaymsg") {
            if let Some(out) = run_capture("swaymsg", &["-t", "get_tree"]).await {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
                    if let Some(c) = sway_focused_class(&v) {
                        return Some(c);
                    }
                }
            }
        }
        if have("kdotool") {
            if let Some(out) = run_capture("kdotool", &["getactivewindow", "getwindowclassname"]).await {
                return Some(out.trim().to_string());
            }
        }
        return None;
    }
    tokio::task::spawn_blocking(x11_active_class).await.ok().flatten()
}

fn sway_focused_class(node: &serde_json::Value) -> Option<String> {
    if node.get("focused").and_then(|f| f.as_bool()) == Some(true) {
        if let Some(id) = node.get("app_id").and_then(|a| a.as_str()) {
            return Some(id.to_string());
        }
        if let Some(c) = node.get("window_properties").and_then(|w| w.get("class")).and_then(|c| c.as_str()) {
            return Some(c.to_string());
        }
    }
    for key in ["nodes", "floating_nodes"] {
        if let Some(children) = node.get(key).and_then(|n| n.as_array()) {
            for c in children {
                if let Some(found) = sway_focused_class(c) {
                    return Some(found);
                }
            }
        }
    }
    None
}

pub async fn auto_shortcut() -> Shortcut {
    match focused_window_class().await {
        Some(c) if is_terminal_class(&c) => {
            debug!("focused window {c:?} looks like a terminal; pasting with Ctrl+Shift+V");
            Shortcut::CtrlShiftV
        }
        _ => Shortcut::CtrlV,
    }
}

// ── Sending it ────────────────────────────────────────────────────────────────

pub async fn send_paste(shortcut: Shortcut) -> Result<()> {
    if wayland_session() {
        if have("wtype") {
            let args: &[&str] = match shortcut {
                Shortcut::CtrlV => &["-M", "ctrl", "v", "-m", "ctrl"],
                Shortcut::CtrlShiftV => &["-M", "ctrl", "-M", "shift", "v", "-m", "shift", "-m", "ctrl"],
                Shortcut::ShiftInsert => &["-M", "shift", "-k", "Insert", "-m", "shift"],
            };
            if run("wtype", args).await {
                return Ok(());
            }
            debug!("wtype could not send the paste shortcut");
        }
        if have("ydotool") {
            // evdev codes: 29 Ctrl, 42 Shift, 47 V, 110 Insert.
            let args: &[&str] = match shortcut {
                Shortcut::CtrlV => &["key", "29:1", "47:1", "47:0", "29:0"],
                Shortcut::CtrlShiftV => &["key", "29:1", "42:1", "47:1", "47:0", "42:0", "29:0"],
                Shortcut::ShiftInsert => &["key", "42:1", "110:1", "110:0", "42:0"],
            };
            if run("ydotool", args).await {
                return Ok(());
            }
            debug!("ydotool could not send the paste shortcut");
        }
        warn!(
            "no Wayland tool (wtype, ydotool) could send the paste shortcut; trying X11, \
             which only reaches applications running under XWayland"
        );
    }

    if std::env::var_os("DISPLAY").is_some() {
        match tokio::task::spawn_blocking(move || x11_send(shortcut)).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(e)) => debug!("native X11 key events failed: {e:#}"),
            Err(e) => debug!("native X11 key task failed: {e}"),
        }
        if have("xdotool") {
            let chord = match shortcut {
                Shortcut::CtrlV => "ctrl+v",
                Shortcut::CtrlShiftV => "ctrl+shift+v",
                Shortcut::ShiftInsert => "shift+Insert",
            };
            if run("xdotool", &["key", "--clearmodifiers", chord]).await {
                return Ok(());
            }
        }
    }
    bail!("no tool could send the paste shortcut (tried wtype, ydotool, X11/XTEST, xdotool)")
}

// ── Native X11 ────────────────────────────────────────────────────────────────

fn connect() -> Result<(RustConnection, Window)> {
    let (conn, screen) = RustConnection::connect(None)?;
    let root = conn.setup().roots[screen].root;
    Ok((conn, root))
}

/// WM_CLASS ("instance\0class\0") of the focused top-level window.
pub fn x11_active_class() -> Option<String> {
    let (conn, root) = connect().ok()?;
    let intern = |n: &str| conn.intern_atom(false, n.as_bytes()).ok()?.reply().ok().map(|r| r.atom);

    // EWMH first: the window manager says which client is active.
    let mut win: Option<Window> = None;
    if let Some(active) = intern("_NET_ACTIVE_WINDOW") {
        if let Some(r) = conn.get_property(false, root, active, AtomEnum::WINDOW, 0, 1).ok()?.reply().ok() {
            if r.value.len() >= 4 {
                let w = u32::from_ne_bytes([r.value[0], r.value[1], r.value[2], r.value[3]]);
                if w != 0 {
                    win = Some(w);
                }
            }
        }
    }
    // Otherwise the input focus, walking up to the top-level that has a class.
    let mut candidate = match win {
        Some(w) => w,
        None => conn.get_input_focus().ok()?.reply().ok()?.focus,
    };
    for _ in 0..10 {
        if candidate == 0 || candidate == root {
            break;
        }
        if let Some(class) = wm_class(&conn, candidate) {
            return Some(class);
        }
        candidate = conn.query_tree(candidate).ok()?.reply().ok()?.parent;
    }
    None
}

fn wm_class(conn: &RustConnection, win: Window) -> Option<String> {
    let r = conn.get_property(false, win, AtomEnum::WM_CLASS, AtomEnum::STRING, 0, 256).ok()?.reply().ok()?;
    if r.value.is_empty() {
        return None;
    }
    // "instance\0class\0" — match against both: either may name the program.
    let parts: Vec<String> = r
        .value
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

struct Keys {
    ctrl: u8,
    shift: u8,
    v: u8,
    insert: u8,
    /// Keycodes that are modifiers, so any the user is holding can be released.
    modifiers: Vec<u8>,
}

fn keycode_for(map: &[u32], per: usize, min: u8, wanted: &[u32], fallback: u8) -> u8 {
    if per > 0 {
        for (i, chunk) in map.chunks(per).enumerate() {
            if chunk.iter().any(|k| wanted.contains(k)) {
                return min.saturating_add(i as u8);
            }
        }
    }
    fallback
}

fn read_keys(conn: &RustConnection) -> Keys {
    let setup = conn.setup();
    let (min, max) = (setup.min_keycode, setup.max_keycode);
    let mapping = conn
        .get_keyboard_mapping(min, max - min + 1)
        .ok()
        .and_then(|c| c.reply().ok());
    let (map, per) = match &mapping {
        Some(m) => (m.keysyms.as_slice(), m.keysyms_per_keycode as usize),
        None => (&[][..], 0),
    };
    let modifiers = conn
        .get_modifier_mapping()
        .ok()
        .and_then(|c| c.reply().ok())
        .map(|m| m.keycodes.into_iter().filter(|k| *k != 0).collect())
        .unwrap_or_default();
    Keys {
        ctrl: keycode_for(map, per, min, &[XK_CONTROL_L], FALLBACK_CTRL),
        shift: keycode_for(map, per, min, &[XK_SHIFT_L], FALLBACK_SHIFT),
        v: keycode_for(map, per, min, &[XK_V_LOWER, XK_V_UPPER], FALLBACK_V),
        insert: keycode_for(map, per, min, &[XK_INSERT], FALLBACK_INSERT),
        modifiers,
    }
}

/// Press the shortcut with the XTEST extension: real key events, delivered to
/// whichever X11 client has focus.
pub fn x11_send(shortcut: Shortcut) -> Result<()> {
    let (conn, root) = connect()?;
    conn.xtest_get_version(2, 1)
        .map_err(|e| anyhow!("XTEST unavailable: {e}"))?
        .reply()
        .map_err(|e| anyhow!("XTEST unavailable: {e}"))?;
    let keys = read_keys(&conn);

    let fake = |kind: u8, code: u8| -> Result<()> {
        conn.xtest_fake_input(kind, code, 0, root, 0, 0, 0)?;
        Ok(())
    };

    // Modifiers still held down (typically the dictation hotkey) would turn
    // this into a different shortcut.
    if let Ok(km) = conn.query_keymap()?.reply() {
        for code in &keys.modifiers {
            let byte = km.keys[(*code / 8) as usize];
            if byte & (1 << (*code % 8)) != 0 {
                fake(KEY_RELEASE_EVENT, *code)?;
            }
        }
    }

    let sequence: Vec<(u8, u8)> = match shortcut {
        Shortcut::CtrlV => vec![
            (KEY_PRESS_EVENT, keys.ctrl), (KEY_PRESS_EVENT, keys.v),
            (KEY_RELEASE_EVENT, keys.v), (KEY_RELEASE_EVENT, keys.ctrl),
        ],
        Shortcut::CtrlShiftV => vec![
            (KEY_PRESS_EVENT, keys.ctrl), (KEY_PRESS_EVENT, keys.shift), (KEY_PRESS_EVENT, keys.v),
            (KEY_RELEASE_EVENT, keys.v), (KEY_RELEASE_EVENT, keys.shift), (KEY_RELEASE_EVENT, keys.ctrl),
        ],
        Shortcut::ShiftInsert => vec![
            (KEY_PRESS_EVENT, keys.shift), (KEY_PRESS_EVENT, keys.insert),
            (KEY_RELEASE_EVENT, keys.insert), (KEY_RELEASE_EVENT, keys.shift),
        ],
    };
    for (kind, code) in sequence {
        fake(kind, code)?;
    }
    // A round trip, so the events are on their way before this returns.
    conn.get_input_focus()?.reply()?;
    std::thread::sleep(Duration::from_millis(5));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminals_are_recognised_by_class() {
        for c in ["gnome-terminal-server", "Gnome-terminal", "Alacritty", "kitty", "org.wezfurlong.wezterm",
                  "XTerm", "xterm xterm", "st", "foot", "com.mitchellh.ghostty", "org.kde.konsole"] {
            assert!(is_terminal_class(c), "{c} should be a terminal");
        }
        for c in ["firefox", "Navigator firefox", "Code", "slack", "libreoffice-writer", "Gedit"] {
            assert!(!is_terminal_class(c), "{c} should not be a terminal");
        }
    }

    #[test]
    fn sway_tree_focus_is_found_through_nesting() {
        let tree = serde_json::json!({
            "focused": false,
            "nodes": [{ "focused": false, "nodes": [{ "focused": true, "app_id": "foot" }] }],
            "floating_nodes": []
        });
        assert_eq!(sway_focused_class(&tree).as_deref(), Some("foot"));
    }

    #[test]
    fn keycodes_come_from_the_keymap_and_fall_back_to_evdev() {
        // min keycode 8; keycode 10 holds 'v'.
        let map = [0u32, 0, 0, 0, XK_V_LOWER, XK_V_UPPER];
        assert_eq!(keycode_for(&map, 2, 8, &[XK_V_LOWER], 55), 8 + 2);
        assert_eq!(keycode_for(&map, 2, 8, &[XK_INSERT], 118), 118);
        assert_eq!(keycode_for(&[], 0, 8, &[XK_V_LOWER], 55), 55);
    }
}
