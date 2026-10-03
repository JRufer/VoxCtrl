//! Sending the paste shortcut through the `org.freedesktop.portal.RemoteDesktop`
//! portal.
//!
//! On Wayland a program cannot press keys on its own: there is no `XTEST`, and
//! `wtype` needs a virtual-keyboard protocol that GNOME and KDE do not offer.
//! The RemoteDesktop portal is the sanctioned route on those desktops: the
//! compositor asks the user once ("VoxCtrl wants to control your keyboard"),
//! then lets the app send key events to whatever has focus — native Wayland
//! windows included.
//!
//! The session is created on first use and kept open. The portal hands back a
//! restore token with the grant, which is saved so later runs of VoxCtrl are
//! not asked again.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use ashpd::desktop::{
    remote_desktop::{DeviceType, KeyState, RemoteDesktop, SelectDevicesOptions},
    PersistMode, Session,
};
use tracing::{debug, info, warn};
use voxctrl_winput::Shortcut;

// evdev key codes — the portal takes these, not X11 keycodes.
const KEY_LEFTCTRL: i32 = 29;
const KEY_LEFTSHIFT: i32 = 42;
const KEY_V: i32 = 47;
const KEY_INSERT: i32 = 110;

/// The application id VoxCtrl declares to the desktop portal (the same one the
/// shortcuts portal uses).
const APP_ID: &str = "ai.voxctrl.app";

/// How long the user gets to answer the permission dialog.
const PROMPT_TIMEOUT: Duration = Duration::from_secs(60);

struct Live {
    proxy: RemoteDesktop,
    session: Session<RemoteDesktop>,
}

static LIVE: tokio::sync::Mutex<Option<Live>> = tokio::sync::Mutex::const_new(None);

/// Set once the user declines (or the desktop has no such portal), so every
/// later dictation does not raise the dialog again. Cleared by restarting.
static UNAVAILABLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn token_path() -> Option<std::path::PathBuf> {
    Some(dirs::config_dir()?.join("voxctrl").join("portal-keyboard-token"))
}

fn load_token() -> Option<String> {
    let t = std::fs::read_to_string(token_path()?).ok()?;
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn save_token(token: &str) {
    let Some(path) = token_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::write(&path, token).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/// Open a session and obtain keyboard access, showing the permission dialog if
/// there is no saved grant.
async fn connect() -> Result<Live> {
    // Declared before the first portal call on the shared connection, and only
    // accepted once per connection — so "already registered" (the shortcuts
    // portal got there first) is expected and fine.
    if let Ok(id) = ashpd::AppID::try_from(APP_ID) {
        if let Err(e) = ashpd::register_host_app(id).await {
            debug!("host app registration for RemoteDesktop: {e}");
        }
    }

    let proxy = RemoteDesktop::new().await.context("the RemoteDesktop portal is not available")?;
    let devices = proxy.available_device_types().await.context("asking the portal for devices")?;
    if !devices.contains(DeviceType::Keyboard) {
        bail!("the RemoteDesktop portal offers no keyboard");
    }

    let session = proxy.create_session(Default::default()).await.context("creating a RemoteDesktop session")?;
    let token = load_token();
    proxy
        .select_devices(
            &session,
            SelectDevicesOptions::default()
                .set_devices(enumflags2::BitFlags::from(DeviceType::Keyboard))
                .set_persist_mode(PersistMode::ExplicitlyRevoked)
                .set_restore_token(token.as_deref()),
        )
        .await?
        .response()
        .context("selecting the keyboard")?;

    if token.is_none() {
        info!("asking the desktop for permission to send keyboard input (needed to paste into Wayland windows)");
    }
    let selected = proxy
        .start(&session, None, Default::default())
        .await?
        .response()
        .context("the desktop did not grant keyboard access")?;
    if !selected.devices().contains(DeviceType::Keyboard) {
        bail!("keyboard access was not granted");
    }
    if let Some(t) = selected.restore_token() {
        save_token(t);
    }
    Ok(Live { proxy, session })
}

async fn tap(live: &Live, code: i32, state: KeyState) -> Result<()> {
    live.proxy
        .notify_keyboard_keycode(&live.session, code, state, Default::default())
        .await
        .map_err(|e| anyhow!("{e}"))
}

async fn press(live: &Live, shortcut: Shortcut) -> Result<()> {
    let (mods, key): (&[i32], i32) = match shortcut {
        Shortcut::CtrlV => (&[KEY_LEFTCTRL], KEY_V),
        Shortcut::CtrlShiftV => (&[KEY_LEFTCTRL, KEY_LEFTSHIFT], KEY_V),
        Shortcut::ShiftInsert => (&[KEY_LEFTSHIFT], KEY_INSERT),
    };
    // A beat between events: compositors process them in order, but some
    // applications drop a chord whose modifier and key land in one frame.
    let gap = || tokio::time::sleep(Duration::from_millis(8));
    for m in mods {
        tap(live, *m, KeyState::Pressed).await?;
    }
    gap().await;
    tap(live, key, KeyState::Pressed).await?;
    gap().await;
    tap(live, key, KeyState::Released).await?;
    gap().await;
    for m in mods.iter().rev() {
        tap(live, *m, KeyState::Released).await?;
    }
    Ok(())
}

/// Make sure there is a session (raising the permission dialog if needed),
/// without sending anything. Called before the clipboard is borrowed, so a
/// dialog does not leave the user's clipboard held while it waits.
pub async fn prepare() -> Result<()> {
    if UNAVAILABLE.load(std::sync::atomic::Ordering::Relaxed) {
        bail!("keyboard access through the portal was declined or is unavailable");
    }
    let mut guard = LIVE.lock().await;
    if guard.is_some() {
        return Ok(());
    }
    match tokio::time::timeout(PROMPT_TIMEOUT, connect()).await {
        Ok(Ok(live)) => {
            *guard = Some(live);
            Ok(())
        }
        Ok(Err(e)) => {
            UNAVAILABLE.store(true, std::sync::atomic::Ordering::Relaxed);
            warn!(
                "RemoteDesktop portal unavailable ({e:#}); pasting into native Wayland windows will not \
                 work until VoxCtrl is restarted and the permission is granted (or install ydotool)"
            );
            Err(e)
        }
        Err(_) => {
            UNAVAILABLE.store(true, std::sync::atomic::Ordering::Relaxed);
            warn!("the keyboard-access permission dialog was not answered in time");
            bail!("timed out waiting for keyboard permission")
        }
    }
}

/// Send the paste shortcut through the portal.
pub async fn send_paste(shortcut: Shortcut) -> Result<()> {
    prepare().await?;
    let mut guard = LIVE.lock().await;
    let live = guard.as_ref().ok_or_else(|| anyhow!("no portal session"))?;
    match press(live, shortcut).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // The session may have been revoked or closed by the compositor.
            // Drop it; the next attempt builds a new one (the saved grant
            // means no dialog), and this one reports failure so another route
            // can be tried.
            debug!("portal key events failed ({e:#}); dropping the session");
            if let Some(dead) = guard.take() {
                let _ = dead.session.close().await;
            }
            Err(e)
        }
    }
}
