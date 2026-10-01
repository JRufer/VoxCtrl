//! Global-shortcut status, diagnostics and desktop registration.

use std::sync::Arc;

use tauri::State;
use tracing::info;

use crate::state::AppState;

/// Tell the gesture handler to silently drop any incoming hotkey events.
/// Called when the user opens the keybind recorder in Settings so they cannot
/// accidentally trigger dictation while pressing keys for a new binding.
#[tauri::command]
pub async fn set_hotkeys_inhibited(
    inhibited: bool,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    state.set_hotkeys_inhibited(inhibited);
    info!("Hotkeys inhibited: {inhibited}");
    Ok(())
}

#[derive(serde::Serialize, Clone)]
pub struct HotkeyStatusPayload {
    /// Global shortcuts can fire right now.
    pub is_active: bool,
    /// Which mechanism is delivering them: `portal`, `evdev`, `windows_hook`,
    /// `starting` or `none`.
    pub backend: String,
    /// VoxCtrl receives only its own shortcuts and can read no keystrokes.
    /// True on the portal and the Windows hook; false on the evdev fallback.
    pub is_private: bool,
    /// Why the desktop portal is not in use, when it is not.
    pub portal_error: Option<String>,
    /// The portal exists and refused VoxCtrl, rather than being absent. Needs
    /// different advice: switching desktops would not help.
    pub portal_refused: bool,
    /// What the compositor actually bound, which may differ from what VoxCtrl
    /// asked for — the user gets the final say in the portal's own dialog.
    pub shortcuts: Vec<voxctrl_hotkeys::BoundShortcut>,
    /// Gesture styles the running backend can actually deliver, as the same
    /// snake_case names the bindings file uses. The settings UI offers exactly
    /// these, so a user is never shown a gesture that silently does nothing.
    pub supported_gestures: Vec<voxctrl_routing::GestureType>,
    /// Why the X11 backend was not used, when it was not. Distinct from
    /// `portal_error`: a Wayland Cinnamon user needs both reasons to make sense
    /// of why neither worked.
    pub x11_error: Option<String>,
    /// `wayland`, `x11` or whatever `XDG_SESSION_TYPE` says.
    pub session_type: String,
    /// `/dev/input/event*` nodes present, and how many VoxCtrl could open.
    /// Only meaningful for the evdev fallback.
    pub devices_total: u32,
    pub devices_readable: u32,
    /// The user has to do something outside VoxCtrl for shortcuts to work.
    pub needs_attention: bool,
    /// One-line, human-readable explanation of the state above.
    pub detail: String,
    /// KDE registers portal shortcuts into System Settings in a *disabled*
    /// state — the user must open Shortcuts, tick each one, and click Apply
    /// before it fires. This is a confirmed upstream xdg-desktop-portal-kde
    /// bug (bugs.kde.org #483639), not something VoxCtrl's registration got
    /// wrong, and the portal protocol has no "enabled" bit for VoxCtrl to
    /// check — so this is a standing warning on KDE + portal, not a detected
    /// fact about any particular shortcut.
    pub needs_manual_enable: bool,
    /// What to tell the user about `needs_manual_enable`, and how to fix it.
    /// `None` when `needs_manual_enable` is false.
    pub manual_enable_hint: Option<String>,
    /// Running on Linux Mint's Cinnamon or MATE desktop environment.
    pub is_mint_desktop: bool,
    /// VoxCtrl's native D-Bus shortcut is registered in Mint's gsettings registry.
    pub mint_shortcut_registered: bool,
    /// Windows only: the focused window belongs to a process elevated above
    /// VoxCtrl's own, so its keyboard hook cannot see keys typed there. UIPI,
    /// not a failure — the hook stays installed and would otherwise look
    /// indistinguishable from "everything is fine".
    pub elevated_window_focused: bool,
}

/// `XDG_CURRENT_DESKTOP` is a colon-separated list (e.g. `ubuntu:GNOME`); any
/// entry can match. `KDE_FULL_SESSION` is a legacy fallback Plasma still sets
/// when the desktop-name entry is missing or non-standard.
#[cfg(target_os = "linux")]
fn desktop_environment() -> Option<String> {
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if current.split(':').any(|d| d.eq_ignore_ascii_case("KDE")) {
        return Some("KDE".to_string());
    }
    if std::env::var_os("KDE_FULL_SESSION").is_some() {
        return Some("KDE".to_string());
    }
    if current.split(':').any(|d| d.eq_ignore_ascii_case("GNOME")) {
        return Some("GNOME".to_string());
    }
    current
        .split(':')
        .find(|d| !d.is_empty())
        .map(|d| d.to_string())
}

#[cfg(not(target_os = "linux"))]
fn desktop_environment() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn session_type() -> String {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "wayland".to_string()
    } else if std::env::var_os("DISPLAY").is_some() {
        "x11".to_string()
    } else {
        std::env::var("XDG_SESSION_TYPE").unwrap_or_else(|_| "unknown".to_string())
    }
}

#[cfg(not(target_os = "linux"))]
fn session_type() -> String {
    std::env::consts::OS.to_string()
}

/// Returns `(total, readable)` counts of `/dev/input/event*` nodes.
///
/// Only used to explain the evdev fallback. On the portal path VoxCtrl opens
/// none of these.
#[cfg(target_os = "linux")]
fn count_input_devices() -> (u32, u32) {
    let Ok(entries) = std::fs::read_dir("/dev/input") else {
        return (0, 0);
    };
    let mut total = 0;
    let mut readable = 0;
    for entry in entries.flatten() {
        let is_event = entry
            .file_name()
            .to_str()
            .map(|n| n.starts_with("event"))
            .unwrap_or(false);
        if !is_event {
            continue;
        }
        total += 1;
        if std::fs::File::open(entry.path()).is_ok() {
            readable += 1;
        }
    }
    (total, readable)
}

#[cfg(not(target_os = "linux"))]
fn count_input_devices() -> (u32, u32) {
    (0, 0)
}

/// How VoxCtrl is receiving global shortcuts, and what — if anything — is
/// stopping it.
///
/// This replaced a udev/`input`-group audit. VoxCtrl no longer configures
/// keyboard access at all, so the question is no longer "did our setup run?"
/// but "does this desktop offer the shortcuts portal, and if not, can we fall
/// back to access the user has already granted themselves?".
pub fn hotkey_status(health: &voxctrl_hotkeys::ListenerHealth) -> HotkeyStatusPayload {
    if let Ok(override_val) = std::env::var("VOXCTRL_TEST_HOTKEY_STATUS") {
        if let Some(payload) = test_override(&override_val) {
            return payload;
        }
    }

    let backend = health.backend();
    let desktop = desktop_environment();
    let is_mint_desktop = crate::mint_shortcuts::is_mint_desktop();
    let mint_shortcut_registered = if is_mint_desktop {
        crate::mint_shortcuts::is_mint_shortcut_registered()
    } else {
        false
    };

    // A Mint shortcut that is registered and bound is a working backend even if
    // the listener never claimed one — the desktop, not VoxCtrl, is holding it.
    let effective_backend = if backend == voxctrl_hotkeys::Backend::None && mint_shortcut_registered
    {
        voxctrl_hotkeys::Backend::MintDbus
    } else {
        backend
    };

    let needs_manual_enable = backend == voxctrl_hotkeys::Backend::Portal
        && desktop.as_deref() == Some("KDE");
    let (devices_total, devices_readable) = match effective_backend {
        // None of these open an input device, so a device count would only
        // invite the user to fix a permission problem they do not have.
        voxctrl_hotkeys::Backend::Portal
        | voxctrl_hotkeys::Backend::WindowsHook
        | voxctrl_hotkeys::Backend::X11
        | voxctrl_hotkeys::Backend::MintDbus => (0, 0),
        _ => count_input_devices(),
    };
    let shortcuts = health.bound_shortcuts();

    let detail = match effective_backend {
        voxctrl_hotkeys::Backend::Portal => {
            let unbound = shortcuts.iter().filter(|s| !s.bound).count();
            if unbound > 0 {
                format!(
                    "Your desktop registered VoxCtrl's shortcuts, but {unbound} of {} could \
                     not be bound. Pick different keys for those, or set them in your \
                     desktop's own shortcut settings.",
                    shortcuts.len()
                )
            } else {
                "Your desktop is handling VoxCtrl's global shortcuts. VoxCtrl cannot read \
                 your keyboard — it is only told when its own shortcut fires."
                    .to_string()
            }
        }
        voxctrl_hotkeys::Backend::WindowsHook if health.elevated_window_focused() => (
            "VoxCtrl cannot see key presses right now: the focused window is running as \
             administrator (Task Manager, an elevated terminal, an installer), and Windows \
             blocks VoxCtrl's keyboard hook from receiving keys while such a window has \
             focus. Switch back to a window that is not running as administrator and your \
             shortcuts will work again."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::WindowsHook => (
            "VoxCtrl is receiving shortcuts through a Windows low-level keyboard hook.              Every gesture style works, including bare modifiers, and no permission setup              was needed — but in this mode every keystroke passes through VoxCtrl. Keys are              matched against your shortcuts and discarded; nothing is stored or sent              anywhere. Windows does not deliver keys to this hook while an elevated              application has focus, or on the secure desktop (the UAC prompt and the lock              screen), so shortcuts do not fire there."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::X11 => (
            "Your desktop has no global-shortcuts portal, so VoxCtrl is reading X11 key \
             events directly. Every gesture style works, including bare modifiers, and no \
             permission setup was needed — but in this mode every keystroke passes through \
             VoxCtrl."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::MintDbus => (
            "Your desktop is handling VoxCtrl's shortcuts through its own keyboard settings. \
             VoxCtrl cannot read your keyboard. This route only carries a press, never a \
             release, so it can serve tap-to-start/tap-to-stop bindings and no other gesture."
        )
        .to_string(),
        voxctrl_hotkeys::Backend::Evdev => format!(
            "This desktop does not offer the global-shortcuts portal, so VoxCtrl is reading \
             input devices directly ({devices_readable} of {devices_total} readable). That \
             works, but it means every keystroke passes through VoxCtrl."
        ),
        voxctrl_hotkeys::Backend::Starting => "Starting the shortcut listener…".to_string(),
        voxctrl_hotkeys::Backend::None if health.portal_refused() => {
            "Global shortcuts require approval from your desktop. The system prompt was \
             closed or declined before shortcuts were registered — click Approve Shortcuts \
             below to display the prompt and confirm your keybinds."
                .to_string()
        }
        voxctrl_hotkeys::Backend::None => {
            if is_mint_desktop {
                "This desktop (Linux Mint) does not provide the XDG global-shortcuts portal, but supports registering native custom shortcuts via System Settings / D-Bus."
                    .to_string()
            } else if devices_total == 0 {
                "No global-shortcuts portal and no input devices were found, so VoxCtrl \
                 cannot receive shortcuts on this system."
                    .to_string()
            } else {
                format!(
                    "This desktop does not provide the XDG global-shortcuts portal, so \
                     VoxCtrl has no way to receive its shortcuts. VoxCtrl will not grant \
                     itself keyboard access to work around it: doing so would let every \
                     program you run read everything you type. None of the {devices_total} \
                     input devices on this system is readable."
                )
            }
        }
    };

    let is_active = health.is_active() || (is_mint_desktop && mint_shortcut_registered);

    HotkeyStatusPayload {
        is_active,
        backend: match effective_backend {
            voxctrl_hotkeys::Backend::Portal => "portal",
            voxctrl_hotkeys::Backend::X11 => "x11",
            voxctrl_hotkeys::Backend::MintDbus => "mint_dbus",
            voxctrl_hotkeys::Backend::Evdev => "evdev",
            voxctrl_hotkeys::Backend::WindowsHook => "windows_hook",
            voxctrl_hotkeys::Backend::Starting => "starting",
            voxctrl_hotkeys::Backend::None => "none",
        }
        .to_string(),
        is_private: health.is_private() || (is_mint_desktop && mint_shortcut_registered),
        portal_error: health.portal_error(),
        portal_refused: health.portal_refused(),
        shortcuts,
        supported_gestures: effective_backend.gestures().to_vec(),
        x11_error: health.x11_error(),
        session_type: session_type(),
        devices_total,
        devices_readable,
        needs_attention: !is_active,
        detail,
        needs_manual_enable,
        manual_enable_hint: needs_manual_enable.then(|| {
            "KDE registers VoxCtrl's shortcuts as disabled until you turn them on yourself: \
             open Shortcuts, find VoxCtrl, tick the box next to each shortcut, and click \
             Apply. This is a known KDE bug (xdg-desktop-portal-kde #483639), not something \
             VoxCtrl's setup missed — the portal gives VoxCtrl no way to tell whether a \
             shortcut is enabled, so this step cannot be automated or skipped."
                .to_string()
        }),
        is_mint_desktop,
        mint_shortcut_registered,
        elevated_window_focused: health.elevated_window_focused(),
    }
}

/// Developer/test override so the setup window can be exercised in every state
/// without a desktop session to match.
fn test_override(value: &str) -> Option<HotkeyStatusPayload> {
    let base = HotkeyStatusPayload {
        is_active: true,
        backend: "portal".to_string(),
        is_private: true,
        portal_error: None,
        portal_refused: false,
        shortcuts: Vec::new(),
        supported_gestures: voxctrl_hotkeys::Backend::Portal.gestures().to_vec(),
        x11_error: None,
        session_type: "wayland".to_string(),
        devices_total: 0,
        devices_readable: 0,
        needs_attention: false,
        detail: "Your desktop is handling VoxCtrl's global shortcuts.".to_string(),
        needs_manual_enable: false,
        manual_enable_hint: None,
        is_mint_desktop: false,
        mint_shortcut_registered: false,
        elevated_window_focused: false,
    };
    match value {
        "portal" => Some(base),
        "mint" => Some(HotkeyStatusPayload {
            is_active: false,
            backend: "none".to_string(),
            is_private: false,
            portal_error: Some("no such interface".to_string()),
            is_mint_desktop: true,
            mint_shortcut_registered: false,
            needs_attention: true,
            detail: "This desktop (Linux Mint) does not provide the XDG global-shortcuts portal, but supports registering native custom shortcuts via System Settings / D-Bus.".to_string(),
            ..base
        }),
        "mint_registered" => Some(HotkeyStatusPayload {
            is_active: true,
            backend: "mint_dbus".to_string(),
            is_private: true,
            portal_error: Some("no such interface".to_string()),
            is_mint_desktop: true,
            mint_shortcut_registered: true,
            needs_attention: false,
            detail: "Linux Mint native desktop shortcut (Ctrl+Alt+Space) is registered in System Settings and triggers VoxCtrl over D-Bus.".to_string(),
            ..base
        }),
        "kde_manual_enable" => Some(HotkeyStatusPayload {
            needs_manual_enable: true,
            manual_enable_hint: Some(
                "KDE registers VoxCtrl's shortcuts as disabled until you turn them on \
                 yourself: open Shortcuts, find VoxCtrl, tick the box next to each \
                 shortcut, and click Apply."
                    .to_string(),
            ),
            ..base
        }),
        "evdev" => Some(HotkeyStatusPayload {
            backend: "evdev".to_string(),
            is_private: false,
            portal_error: Some("no such interface".to_string()),
            devices_total: 6,
            devices_readable: 6,
            detail: "VoxCtrl is reading input devices directly.".to_string(),
            ..base
        }),
        "none" => Some(HotkeyStatusPayload {
            is_active: false,
            backend: "none".to_string(),
            is_private: false,
            portal_error: Some("no such interface".to_string()),
            devices_total: 6,
            devices_readable: 0,
            needs_attention: true,
            detail: "This desktop does not provide the XDG global-shortcuts portal.".to_string(),
            ..base
        }),
        "refused" => Some(HotkeyStatusPayload {
            is_active: false,
            backend: "none".to_string(),
            is_private: false,
            portal_error: Some(
                "org.freedesktop.portal.Error.NotAllowed: An app id is required".to_string(),
            ),
            portal_refused: true,
            needs_attention: true,
            detail: "Your desktop has a global-shortcuts portal but refused VoxCtrl's \
                     request for one."
                .to_string(),
            ..base
        }),
        _ => None,
    }
}

/// Launch the desktop's own global-shortcuts settings panel, best-effort.
///
/// Exists for the KDE `needs_manual_enable` case — there is no D-Bus verb that
/// flips a portal shortcut from disabled to enabled, so the fix genuinely is
/// "open this panel yourself". Tries the KDE System Settings module directly
/// first (skips the home screen the user would otherwise have to navigate
/// through by hand, which is the whole point of the button), then falls back
/// to whatever System Settings binary exists, then to GNOME's keyboard panel
/// on the off chance this is ever needed there too.
///
/// Errors only when nothing in the list is installed — a real signal the user
/// is not actually on the desktop this feature assumes, worth surfacing
/// rather than silently doing nothing.
#[tauri::command]
pub async fn open_shortcut_settings() -> Result<(), String> {
    #[cfg(not(target_os = "linux"))]
    {
        return Err("Opening system shortcut settings is only supported on Linux.".to_string());
    }

    #[cfg(target_os = "linux")]
    {
        for (bin, args) in shortcut_settings_candidates() {
            if !crate::installer::command_exists(bin) {
                continue;
            }
            match spawn_shortcut_settings(bin, args) {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("Found `{bin}` but failed to launch it: {e}");
                    continue;
                }
            }
        }

        Err("Could not find a way to open your desktop's shortcut settings automatically. \
             Open System Settings yourself and look for Shortcuts → VoxCtrl."
            .to_string())
    }
}

/// Request or retry global shortcut registration via the XDG desktop portal.
#[tauri::command]
pub async fn retry_portal_shortcuts(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let Some(bindings) = crate::stop_key::listener_bindings_from_disk(&state).await else {
            return Err("Could not read your saved shortcuts, so there is nothing to \
                        register. Check the log for what went wrong reading bindings.toml."
                .to_string());
        };

        let gesture_tx = {
            let gtx_guard = state.hotkey_gesture_tx.lock().await;
            gtx_guard.clone()
        };
        let Some(gesture_tx) = gesture_tx else {
            return Err("Hotkey gesture channel is not available.".to_string());
        };

        let (reloader_tx, reloader_rx) = crossbeam_channel::unbounded();
        {
            let mut reloader = state.hotkey_reloader.lock().await;
            *reloader = Some(reloader_tx);
        }

        voxctrl_hotkeys::retry_portal(
            bindings,
            gesture_tx,
            reloader_rx,
            state.hotkey_health.clone(),
        )
        .await
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = state;
        Ok(())
    }
}

/// Tried in order: the KDE System Settings module directly (skips the home
/// screen the user would otherwise navigate through by hand — the whole point
/// of the button), then whichever System Settings binary exists, then GNOME's
/// keyboard panel on the off chance this is ever needed there too.
#[cfg(target_os = "linux")]
fn shortcut_settings_candidates() -> &'static [(&'static str, &'static [&'static str])] {
    &[
        ("kcmshell6", &["kcm_keys"]),
        ("kcmshell5", &["kcm_keys"]),
        ("systemsettings6", &["kcm_keys"]),
        ("systemsettings", &["kcm_keys"]),
        ("gnome-control-center", &["keyboard"]),
    ]
}

/// Fire-and-forget: these are GUI apps meant to stay open long after this
/// command returns, so there is nothing useful to await here.
///
/// These are host desktop binaries (`kcmshell6`, `systemsettings`,
/// `gnome-control-center`), not part of the bundle, so they are launched
/// through `host_env::host_command` rather than `std::process::Command`
/// directly: inside the AppImage, a plain `Command::new` hands them
/// VoxCtrl's own environment, including an `LD_LIBRARY_PATH` that puts the
/// bundle's libraries first. A host `kcmshell6` linked against the host's
/// `libcurl` then resolves `libssl` from the bundle instead of the host and
/// aborts before it can show a window (see host_env.rs's doc comment for the
/// same failure mode with `gsettings`).
#[cfg(target_os = "linux")]
fn spawn_shortcut_settings(bin: &str, args: &[&str]) -> Result<(), String> {
    #[cfg(test)]
    {
        if std::env::var_os("VOXCTRL_INSTALLER_TEST_MOCK").is_some() {
            return Ok(());
        }
    }
    crate::host_env::host_command(bin)
        .args(args)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Verdict on a key combination the user just recorded.
#[derive(serde::Serialize, Clone)]
pub struct HotkeyKeysCheck {
    /// The combination can be saved as-is.
    pub accepted: bool,
    /// True when a rejection is binding rather than advisory — i.e. shortcuts
    /// are delivered by the desktop portal, which cannot register this. On the
    /// evdev fallback and on Windows, VoxCtrl watches the keys itself and a
    /// bare modifier works fine, so the same combination is merely flagged.
    pub enforced: bool,
    /// The shortcut as the desktop will see it, e.g. `LOGO+space`.
    pub accelerator: Option<String>,
    /// Machine-readable problem: `modifiers_only`, `multiple_keys`,
    /// `unsupported_key` or `empty`.
    pub problem: Option<String>,
    /// What to tell the user, and what to press instead.
    pub message: Option<String>,
}

impl HotkeyKeysCheck {
    fn ok(accelerator: Option<String>) -> Self {
        Self {
            accepted: true,
            enforced: false,
            accelerator,
            problem: None,
            message: None,
        }
    }
}

/// A combination the desktop can bind, plus a warning when *holding* it would
/// cost the rest of the desktop the key.
///
/// A binding here is registered for as long as VoxCtrl runs, and where the
/// compositor owns the grab that registration is exclusive: bare Escape would
/// reach VoxCtrl and nothing else, so menus would stop closing everywhere. It
/// is still the user's call — they may genuinely want Escape to start dictation
/// — so this is advice, not a refusal. The TTS stop key is the case that must
/// not carry this cost silently, and it does not: `stop_key` holds it only
/// while VoxCtrl is speaking.
fn standing_grab_check(
    keys: &[String],
    accelerator: String,
    health: &voxctrl_hotkeys::ListenerHealth,
) -> HotkeyKeysCheck {
    let takes_the_key = voxctrl_hotkeys::is_reserved_for_the_desktop(keys)
        && !health.backend().sees_raw_keys();
    if !takes_the_key {
        return HotkeyKeysCheck::ok(Some(accelerator));
    }
    HotkeyKeysCheck {
        // It genuinely works. Refusing it would be a lie, and the cost is the
        // user's to weigh.
        accepted: true,
        enforced: false,
        accelerator: Some(accelerator),
        problem: Some("reserved_key".to_string()),
        message: Some(
            "Your desktop hands a registered shortcut to VoxCtrl alone, and a binding is \
             held for as long as VoxCtrl runs — so with Escape bound here, an open menu or \
             dialog would stop closing anywhere on your desktop. Add a modifier \
             (Ctrl+Escape) to avoid that. The TTS stop key is safe to leave on Escape: it \
             is held only while VoxCtrl is speaking."
                .to_string(),
        ),
    }
}

/// Can this key combination be registered as a global shortcut?
///
/// The settings UI calls this instead of reimplementing the rules, so the key
/// recorder and the portal registration can never disagree about what is
/// valid. `voxctrl_hotkeys::accelerator` is the single definition.
pub fn check_hotkey_keys_with(
    keys: &[String],
    health: &voxctrl_hotkeys::ListenerHealth,
) -> HotkeyKeysCheck {
    use voxctrl_hotkeys::TriggerProblem;

    let problem = match voxctrl_hotkeys::accelerator(keys) {
        Ok(accelerator) => return standing_grab_check(keys, accelerator, health),
        Err(problem) => problem,
    };

    // Only the backends that hand the grab to the desktop actually cannot
    // deliver these. Blocking them everywhere would break bare-modifier
    // shortcuts on the backends where VoxCtrl watches the keys itself — X11,
    // evdev and the Windows hook — where they work perfectly well.
    let enforced = !health.backend().sees_raw_keys();

    let hint = match problem {
        TriggerProblem::ModifiersOnly => Some(
            "Add a regular key to the combination — Super+Space and Ctrl+Alt+D both work.",
        ),
        TriggerProblem::MultipleKeys => {
            Some("Keep one regular key and use modifiers for the rest.")
        }
        TriggerProblem::UnsupportedKey(_) => Some("Try a letter, number, function or arrow key."),
        TriggerProblem::Empty => None,
    };

    let mut message = if enforced {
        format!("Your desktop cannot register this shortcut: {problem}.")
    } else {
        format!(
            "This works right now, because VoxCtrl is watching the keyboard itself rather \
             than using the desktop's shortcut service. It will stop working if that \
             changes: {problem}."
        )
    };
    if let Some(hint) = hint {
        message.push(' ');
        message.push_str(hint);
    }

    HotkeyKeysCheck {
        // Advisory-only rejections still save: the combination genuinely works
        // on this machine, and refusing it would be a lie.
        accepted: !enforced,
        enforced,
        accelerator: None,
        problem: Some(
            match problem {
                TriggerProblem::Empty => "empty",
                TriggerProblem::ModifiersOnly => "modifiers_only",
                TriggerProblem::MultipleKeys => "multiple_keys",
                TriggerProblem::UnsupportedKey(_) => "unsupported_key",
            }
            .to_string(),
        ),
        message: Some(message),
    }
}

#[tauri::command]
pub async fn check_hotkey_keys(
    keys: Vec<String>,
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyKeysCheck, String> {
    Ok(check_hotkey_keys_with(&keys, &state.hotkey_health))
}

#[tauri::command]
pub async fn check_hotkey_status(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyStatusPayload, String> {
    Ok(hotkey_status(&state.hotkey_health))
}

#[tauri::command]
pub async fn register_mint_shortcut(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<String, String> {
    let result = crate::mint_shortcuts::register_mint_shortcut(None)?;
    state
        .hotkey_health
        .set_backend(voxctrl_hotkeys::Backend::MintDbus);
    Ok(result)
}

#[tauri::command]
pub async fn approve_shortcuts(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyStatusPayload, String> {
    if crate::mint_shortcuts::is_mint_desktop()
        && state.hotkey_health.backend() == voxctrl_hotkeys::Backend::None
    {
        crate::mint_shortcuts::register_mint_shortcut(None)?;
        state
            .hotkey_health
            .set_backend(voxctrl_hotkeys::Backend::MintDbus);
    } else if state.hotkey_health.backend() == voxctrl_hotkeys::Backend::Portal {
        let _ = open_shortcut_settings().await;
    } else {
        let _ = retry_portal_shortcuts(state.clone()).await;
    }
    Ok(hotkey_status(&state.hotkey_health))
}

/// Install the host packages VoxCtrl needs to type text into other windows.
///
/// Never touches keyboard permissions — global shortcuts come from the desktop
/// portal, which needs none.
#[tauri::command]
pub async fn install_system_integration(
    state: tauri::State<'_, std::sync::Arc<crate::state::AppState>>,
) -> Result<HotkeyStatusPayload, String> {
    crate::installer::run_gui_installer().await?;
    Ok(hotkey_status(&state.hotkey_health))
}
