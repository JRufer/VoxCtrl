use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use anyhow::Result;
use tracing::{debug, warn};
pub use voxctrl_winput::Shortcut;

#[cfg(target_os = "linux")]
mod keys_linux;

/// The least time the target gets to read the clipboard before it is handed
/// back. Restoring immediately races the paste: the application reads the
/// clipboard when it processes the shortcut, which can be well after the key
/// event was queued (Electron, remote desktops, VMs).
const PASTE_SETTLE: Duration = Duration::from_millis(300);
/// Where the clipboard backend can see the application read the text (X11),
/// how much longer to wait for a slow application before giving up.
const PASTE_READ_PATIENCE: Duration = Duration::from_millis(2000);
/// Let the application finish using the text once it has read it.
const PASTE_AFTER_READ: Duration = Duration::from_millis(120);

static PASTE_MODE: AtomicBool = AtomicBool::new(true);
/// 0 = auto, 1 = Ctrl+V, 2 = Ctrl+Shift+V, 3 = Shift+Insert.
static PASTE_SHORTCUT: AtomicU8 = AtomicU8::new(0);

/// Choose between pasting (`true`, the default) and typing (`false`) the
/// transcription. Takes effect on the next injection.
pub fn set_paste_mode(on: bool) {
    PASTE_MODE.store(on, Ordering::Relaxed);
}

/// Whether pasting is switched on *and* allowed on this system. Where it is
/// not allowed (see [`paste_unsupported_reason`]) the setting has no effect:
/// text is always typed.
pub fn paste_mode() -> bool {
    PASTE_MODE.load(Ordering::Relaxed) && paste_unsupported_reason().is_none()
}

/// Why pasting is disabled on this system, or `None` when it is available.
///
/// Linux Mint is excluded: pasting does not work reliably there, so the
/// setting is turned off rather than left to fail. Set
/// `VOXCTRL_FORCE_PASTE=1` to override this when testing.
pub fn paste_unsupported_reason() -> Option<&'static str> {
    static REASON: std::sync::OnceLock<Option<&'static str>> = std::sync::OnceLock::new();
    *REASON.get_or_init(|| {
        if std::env::var_os("VOXCTRL_FORCE_PASTE").is_some() {
            return None;
        }
        #[cfg(target_os = "linux")]
        {
            let os_release = std::fs::read_to_string("/etc/os-release")
                .or_else(|_| std::fs::read_to_string("/usr/lib/os-release"))
                .unwrap_or_default();
            if is_linux_mint(&os_release) {
                return Some("Pasting is not supported on Linux Mint, so VoxCtrl types the text instead.");
            }
        }
        None
    })
}

/// Whether an `os-release` file describes Linux Mint (including LMDE, which
/// reports the same ID).
#[cfg(any(target_os = "linux", test))]
fn is_linux_mint(os_release: &str) -> bool {
    os_release.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else { return false };
        let value = value.trim().trim_matches('"').to_ascii_lowercase();
        match key.trim() {
            "ID" => value == "linuxmint",
            "NAME" => value.contains("linux mint"),
            _ => false,
        }
    })
}

/// Set the paste shortcut by name (`"auto"`, `"ctrl+v"`, `"ctrl+shift+v"`,
/// `"shift+insert"`). Anything unrecognised means auto: pick one for the
/// focused application.
pub fn set_paste_shortcut(name: &str) {
    let v = match Shortcut::parse(name) {
        None => 0,
        Some(Shortcut::CtrlV) => 1,
        Some(Shortcut::CtrlShiftV) => 2,
        Some(Shortcut::ShiftInsert) => 3,
    };
    PASTE_SHORTCUT.store(v, Ordering::Relaxed);
}

fn configured_shortcut() -> Option<Shortcut> {
    match PASTE_SHORTCUT.load(Ordering::Relaxed) {
        1 => Some(Shortcut::CtrlV),
        2 => Some(Shortcut::CtrlShiftV),
        3 => Some(Shortcut::ShiftInsert),
        _ => None,
    }
}

/// Inject text into the currently focused window using the best available
/// method for the current platform and display server, honouring
/// [`paste_mode`].
pub async fn inject_text(text: &str) -> Result<()> {
    inject_text_with(text, paste_mode()).await
}

/// As [`inject_text`], with the mode given explicitly.
///
/// Pasting is one shortcut instead of one key event per character, so text
/// sent while focus is somewhere that is not a text field does not turn into
/// hundreds of stray key presses. If the paste cannot be performed the text is
/// typed instead, so a dictation is never silently dropped.
pub async fn inject_text_with(text: &str, paste: bool) -> Result<()> {
    if text.is_empty() {
        return Ok(());
    }
    // Very long text is pasted whichever mode is chosen: typing it makes the
    // target process one message per character.
    // Where pasting is disabled, that includes the long-text case.
    let paste = (paste || voxctrl_winput::prefers_paste(text)) && paste_unsupported_reason().is_none();

    let _one_at_a_time = INJECT_LOCK.lock().await;
    if paste {
        match paste_text(text).await {
            Ok(()) => return Ok(()),
            Err(e) => warn!("paste failed ({e:#}); typing instead"),
        }
    }
    type_text(text).await
}

// ── Paste ─────────────────────────────────────────────────────────────────────

/// How long each clipboard step may take before it is given up on.
///
/// A clipboard operation can block on another program: the application that
/// owns the clipboard may be frozen, slow to render a large item, or a remote
/// session. Without a limit one such application would hang every dictation
/// that follows, with nothing typed or pasted and no error. A step that times
/// out is abandoned (its thread finishes in the background) and the dictation
/// carries on, or falls back to typing.
const SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(4);
const SET_TIMEOUT: Duration = Duration::from_secs(4);
const RESTORE_TIMEOUT: Duration = Duration::from_secs(5);
const SEND_TIMEOUT: Duration = Duration::from_secs(3);

/// One injection at a time. Two dictations in quick succession would
/// otherwise share the clipboard: the second would back up the first one's
/// text as "the user's clipboard" and restore it afterwards.
static INJECT_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn blocking_with_timeout<T, F>(what: &str, limit: Duration, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    match tokio::time::timeout(limit, tokio::task::spawn_blocking(f)).await {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => anyhow::bail!("{what} task failed: {e}"),
        Err(_) => anyhow::bail!("{what} timed out after {limit:?}"),
    }
}

/// Back up the clipboard, put the text on it, press the paste shortcut, wait
/// for the application to take it, and put the clipboard back.
async fn paste_text(text: &str) -> Result<()> {
    let started = std::time::Instant::now();

    // Chosen before the text goes on the clipboard, while the window that will
    // receive the paste is certain to be the one that has focus.
    let shortcut = match configured_shortcut() {
        Some(s) => s,
        None => auto_shortcut().await,
    };

    // 1. Back up. Failing to is not fatal: dropping the dictation is worse
    //    than losing what was copied, but it is logged.
    let saved = match blocking_with_timeout("clipboard backup", SNAPSHOT_TIMEOUT, voxctrl_clipboard::snapshot).await {
        Ok(Ok(s)) => {
            debug!(formats = s.format_count(), elapsed = ?started.elapsed(), "clipboard backed up");
            Some(s)
        }
        Ok(Err(e)) => {
            warn!("could not back up the clipboard ({e:#}); it will not be restored");
            None
        }
        Err(e) => {
            warn!("{e:#}; the clipboard will not be restored");
            None
        }
    };

    // 2. Put the text on the clipboard. If that fails there is nothing to
    //    paste, and the caller types instead.
    let t = text.to_string();
    let held = blocking_with_timeout("setting the clipboard", SET_TIMEOUT, move || voxctrl_clipboard::set_text(&t))
        .await??;
    debug!(elapsed = ?started.elapsed(), "text on the clipboard");

    // 3. Paste.
    let mark = held.mark();
    let sent = match tokio::time::timeout(SEND_TIMEOUT, send_paste(shortcut)).await {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!("sending the paste shortcut timed out")),
    };
    tracing::info!(?shortcut, ok = sent.is_ok(), elapsed = ?started.elapsed(), "paste shortcut sent");

    // 4. Give the application time to read the text.
    tokio::time::sleep(PASTE_SETTLE).await;
    let held = std::sync::Arc::new(held);
    if sent.is_ok() && held.tracks_reads() {
        // Slow applications get as long as they need, up to a limit; one that
        // has already read the text gets a moment to finish with it.
        let h = held.clone();
        let read = tokio::task::spawn_blocking(move || h.wait_read_since(mark, PASTE_READ_PATIENCE))
            .await
            .unwrap_or(false);
        if read {
            tokio::time::sleep(PASTE_AFTER_READ).await;
        }
    }

    // 5. Put the clipboard back — only if it still holds our text: if the
    //    user copied something in the meantime, that is theirs.
    if let Some(saved) = saved {
        let h = held.clone();
        let ours = text.to_string();
        let restored = blocking_with_timeout("clipboard restore", RESTORE_TIMEOUT, move || {
            // Ownership alone is not the test. A clipboard manager, or the
            // compositor bridging Wayland and X11, takes the clipboard over
            // without changing what it holds — and refusing to restore then
            // leaves the dictation on the clipboard for good. So the clipboard
            // is ours to restore if we still own it *or* it still holds our
            // text. If the user copied something else, it holds neither.
            let owned = h.still_current();
            let holds_our_text = owned || voxctrl_clipboard::current_text().as_deref() == Some(ours.as_str());
            if holds_our_text {
                let formats = saved.format_count();
                h.restore(saved).map(|()| Some(formats))
            } else {
                Ok(None)
            }
        })
        .await;
        match restored {
            Ok(Ok(Some(n))) => tracing::info!(formats = n, elapsed = ?started.elapsed(), "clipboard restored"),
            Ok(Ok(None)) => tracing::info!("the clipboard changed during the paste and no longer holds the dictation; left alone"),
            Ok(Err(e)) => warn!("could not restore the clipboard: {e:#}"),
            Err(e) => warn!("{e:#}"),
        }
    }

    sent?;
    tracing::info!(?shortcut, elapsed = ?started.elapsed(), "Injected via paste");
    Ok(())
}

#[cfg(target_os = "linux")]
async fn auto_shortcut() -> Shortcut {
    keys_linux::auto_shortcut().await
}

#[cfg(target_os = "windows")]
async fn auto_shortcut() -> Shortcut {
    tokio::task::spawn_blocking(voxctrl_winput::auto_shortcut)
        .await
        .unwrap_or(Shortcut::CtrlV)
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn auto_shortcut() -> Shortcut {
    Shortcut::CtrlV
}

#[cfg(target_os = "linux")]
async fn send_paste(shortcut: Shortcut) -> Result<()> {
    keys_linux::send_paste(shortcut).await
}

#[cfg(target_os = "windows")]
async fn send_paste(shortcut: Shortcut) -> Result<()> {
    tokio::task::spawn_blocking(move || voxctrl_winput::press_paste(shortcut)).await?
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn send_paste(_: Shortcut) -> Result<()> {
    anyhow::bail!("Text injection not supported on this platform")
}

// ── Typing (the fallback) ─────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
async fn type_text(text: &str) -> Result<()> {
    let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();

    // 1. wtype (Wayland native)
    if wayland && voxctrl_config::find_in_path("wtype").is_some() {
        if run_cmd("wtype", &["--", text]).await {
            debug!("Injected via wtype");
            return Ok(());
        }
        warn!("wtype failed; trying fallback");
    }

    // 2. xdotool (X11 / XWayland)
    //
    // The delay is per character, so it sets how long a transcription takes to
    // appear: at xdotool's own default of 12 ms a 200-character paragraph
    // types for two and a half seconds. 4 ms still leaves a gap far wider than
    // an X client needs to keep up — `wtype`, the Wayland path above, inserts
    // no gap at all — while cutting that wait to a third.
    if voxctrl_config::find_in_path("xdotool").is_some() {
        if run_cmd("xdotool", &["type", "--clearmodifiers", "--delay", "4", "--", text]).await {
            debug!("Injected via xdotool");
            return Ok(());
        }
        warn!("xdotool failed");
    }

    anyhow::bail!("No injection method available (wtype / xdotool)")
}

#[cfg(target_os = "linux")]
async fn run_cmd(bin: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(bin)
        .args(args)
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(target_os = "windows")]
async fn type_text(text: &str) -> Result<()> {
    // `SendInput` is a blocking Win32 call and long text is a lot of events, so
    // it does not belong on an async worker.
    let t = text.to_string();
    tokio::task::spawn_blocking(move || voxctrl_winput::type_text(&t)).await??;
    debug!("Injected via SendInput");
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn type_text(_text: &str) -> Result<()> {
    anyhow::bail!("Text injection not supported on this platform")
}

// ── Desktop notifications ─────────────────────────────────────────────────────

pub fn show_notification(summary: &str, body: &str) {
    let summary = summary.to_string();
    let body = body.to_string();
    // Fire-and-forget; don't block the caller
    std::thread::spawn(move || {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        {
            let _ = notify_rust::Notification::new()
                .summary(&summary)
                .body(&body)
                .timeout(notify_rust::Timeout::Milliseconds(3000))
                .show();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = notify_rust::Notification::new()
                .summary(&summary)
                .body(&body)
                .show();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paste_mode_defaults_on_and_can_be_switched() {
        set_paste_mode(false);
        assert!(!paste_mode());
        set_paste_mode(true);
        assert!(paste_mode());
    }

    #[test]
    fn linux_mint_is_recognised_from_os_release() {
        let mint = "NAME=\"Linux Mint\"\nVERSION=\"22 (Wilma)\"\nID=linuxmint\nID_LIKE=\"ubuntu debian\"\n";
        assert!(is_linux_mint(mint));
        let lmde = "PRETTY_NAME=\"LMDE 6 (faye)\"\nNAME=\"LMDE\"\nID=linuxmint\nID_LIKE=debian\n";
        assert!(is_linux_mint(lmde));
        let ubuntu = "NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n";
        assert!(!is_linux_mint(ubuntu));
        // Ubuntu-derived but not Mint.
        assert!(!is_linux_mint("NAME=\"Pop!_OS\"\nID=pop\nID_LIKE=\"ubuntu debian\"\n"));
    }

    #[test]
    fn the_configured_shortcut_overrides_auto() {
        set_paste_shortcut("ctrl+shift+v");
        assert_eq!(configured_shortcut(), Some(Shortcut::CtrlShiftV));
        set_paste_shortcut("Shift+Insert");
        assert_eq!(configured_shortcut(), Some(Shortcut::ShiftInsert));
        set_paste_shortcut("auto");
        assert_eq!(configured_shortcut(), None);
        set_paste_shortcut("nonsense");
        assert_eq!(configured_shortcut(), None);
    }
}
