use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::Manager;
use crate::commands;
use crate::state::AppState;

/// Set once the Tauri app is built, so background tasks started before it (the
/// hotkey gesture loop) can raise windows.
static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

/// Label of the first-run setup window.
pub const SETUP_WINDOW: &str = "udev-warning";

/// Label of the first-launch setup wizard window.
pub const WIZARD_WINDOW: &str = "wizard";

/// Label of the "a new version is available" window.
pub const UPDATE_WINDOW: &str = "update";

/// The wizard's screens are laid out on a wide stage — two engine cards side by
/// side, eight overlay thumbnails in a row, five voice cards in a row. The
/// widest breakpoint in the step stylesheets is 1200px, and the tallest step
/// (the overlay grid over the position preview) needs a shade under 1000px of
/// height before its footer is pushed off the bottom. So the floor is the size
/// at which every step is known to render at its intended breakpoint, and the
/// window opens a little above it — enough slack for the layout to breathe
/// without the dead space a much bigger window leaves behind. These must stay in step with the `wizard`
/// entry in tauri.conf.json, which is what a fresh install's first launch uses.
pub const WIZARD_WIDTH: f64 = 1280.0;
pub const WIZARD_HEIGHT: f64 = 1057.0;
pub const WIZARD_MIN_WIDTH: f64 = 1140.0;
pub const WIZARD_MIN_HEIGHT: f64 = 900.0;

/// Default and minimum geometry for the Settings window. Its sidebar plus the
/// widest tab body need the width, and the longest tab needs the height before
/// it starts scrolling on first open.
pub const SETTINGS_WIDTH: f64 = 880.0;
pub const SETTINGS_HEIGHT: f64 = 1000.0;
pub const SETTINGS_MIN_WIDTH: f64 = 720.0;
pub const SETTINGS_MIN_HEIGHT: f64 = 640.0;

/// Fraction of the display the window may occupy, leaving room for the title
/// bar and a desktop panel. Height is the tighter of the two: panels are
/// usually horizontal, and the title bar eats from the same axis.
#[allow(dead_code)]
const WIZARD_FIT_W: f64 = 0.94;
#[allow(dead_code)]
const WIZARD_FIT_H: f64 = 0.90;

/// The largest window in the wizard's design proportions that fits the space
/// available, capped at the design size and floored at the size below which the
/// layout stops fitting.
///
/// A fixed design size is only safe at 100% scaling: the same window on a 1080p
/// display at 125% is scaled up in physical pixels, wider and taller than the
/// screen, so the footer with the Continue button ends up past the bottom edge.
/// Sizes are logical pixels, which is what the compositor scales.
///
/// The floor wins over fitting on purpose. A user can move or scroll a window
/// that is slightly too big for their desktop; they cannot unwrap a layout that
/// has dropped to a narrower breakpoint, which is what a smaller window gives
/// them.
pub fn wizard_size_for(available_width: f64, available_height: f64) -> (f64, f64) {
    let aspect = WIZARD_WIDTH / WIZARD_HEIGHT;

    let w = available_width.min(WIZARD_WIDTH);
    let h = available_height.min(WIZARD_HEIGHT);

    // Shrink whichever axis is over-long, so the window keeps its proportions
    // rather than letterboxing the layout it was designed around.
    let (w, h) = if w / h > aspect { (h * aspect, h) } else { (w, w / aspect) };

    (w.max(WIZARD_MIN_WIDTH), h.max(WIZARD_MIN_HEIGHT))
}

/// Resize a window to fit the display it is on, and re-centre it.
#[allow(dead_code)]
fn fit_to_display(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let logical = monitor.size().to_logical::<f64>(monitor.scale_factor());
    let (w, h) = wizard_size_for(logical.width * WIZARD_FIT_W, logical.height * WIZARD_FIT_H);
    let _ = window.set_size(tauri::LogicalSize::new(w, h));
    let _ = window.center();
}

/// Minimum gap between "finish the setup" notifications, so holding a
/// push-to-talk key does not produce a wall of toasts.
pub const SETUP_NOTICE_INTERVAL: Duration = Duration::from_secs(60);

/// How often the setup watcher re-checks the listener. Short enough that a
/// change made elsewhere — the portal coming up, a shortcut reassigned in the
/// desktop's settings — visibly flips the app to working within a couple of
/// seconds.
#[cfg(target_os = "linux")]
pub const SETUP_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Re-alert cadence while nothing can deliver shortcuts at all. In that state
/// no shortcut can reach the app, so this is the only way the user hears about
/// it while they are pressing keys and getting nothing.
#[cfg(target_os = "linux")]
pub const BLIND_ALERT_INTERVAL: Duration = Duration::from_secs(300);

pub fn set_app_handle(handle: tauri::AppHandle) {
    let _ = APP_HANDLE.set(handle);
}

pub fn get_app_handle() -> Option<tauri::AppHandle> {
    APP_HANDLE.get().cloned()
}

/// Show the Settings window, building it if the user has closed it.
///
/// Closing a window destroys it, so every entry point into Settings — the tray,
/// a second launch, the wizard's "Open Settings" — has to be able to make a new
/// one. Geometry is kept in step with the `settings` entry in tauri.conf.json.
pub fn open_settings_window(app: &tauri::AppHandle) -> Result<tauri::WebviewWindow, String> {
    if let Some(existing) = app.get_webview_window("settings") {
        show_and_focus_window(&existing);
        return Ok(existing);
    }

    tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("/settings".into()))
        .title("VoxCtrl Settings")
        .inner_size(SETTINGS_WIDTH, SETTINGS_HEIGHT)
        .min_inner_size(SETTINGS_MIN_WIDTH, SETTINGS_MIN_HEIGHT)
        .center()
        .resizable(true)
        .decorations(true)
        .build()
        .map_err(|e| format!("Could not open Settings: {e}"))
}

/// Show the first-run wizard, building its window if it is no longer there.
///
/// The wizard closes itself when the user finishes, and a closed Tauri window
/// cannot be shown again — so re-opening it has to construct a new one. That is
/// the right behaviour anyway: a re-run should start at step one with a fresh
/// webview, not resume on whatever screen the last run ended on.
///
/// Geometry is kept in step with the `wizard` entry in `tauri.conf.json`, which
/// is what the first launch of a fresh install uses.
pub fn open_wizard_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(WIZARD_WINDOW) {
        let _ = existing.set_size(tauri::LogicalSize::new(WIZARD_WIDTH, WIZARD_HEIGHT));
        let _ = existing.center();
        show_and_focus_window(&existing);
        return Ok(());
    }

    let window = tauri::WebviewWindowBuilder::new(
        app,
        WIZARD_WINDOW,
        tauri::WebviewUrl::App("/wizard".into()),
    )
    .title("VoxCtrl — First-Run Setup")
    .inner_size(WIZARD_WIDTH, WIZARD_HEIGHT)
    .min_inner_size(WIZARD_MIN_WIDTH, WIZARD_MIN_HEIGHT)
    .center()
    .resizable(true)
    .decorations(true)
    .build()
    .map_err(|e| format!("Could not open the setup wizard: {e}"))?;

    let _ = window.set_size(tauri::LogicalSize::new(WIZARD_WIDTH, WIZARD_HEIGHT));
    let _ = window.center();

    Ok(())
}

/// Label of the dictation overlay window.
pub const OVERLAY_WINDOW: &str = "overlay";
// The overlay's actual content (the widest built-in style, the 440px-wide
// Neon Spectrum panel) is centered via flex inside this window with real
// margin to spare, so it stays comfortably clear of the window edges. On
// Linux this has to absorb the rendered size sometimes coming out a few px
// smaller than requested: forcing GDK_BACKEND=x11 (see lib.rs) makes GDK
// approximate the display's real scale factor — often fractional under
// Wayland — by rounding to an integer X11 scale, which showed up as the
// window edge clipping into the Voice Card style's rounded corners before
// this was widened.
const OVERLAY_WIDTH: f64 = 1184.0;
const OVERLAY_HEIGHT: f64 = 444.0;

/// Build (or fetch) the dictation overlay: a transparent, frameless,
/// always-on-top, click-through `WebviewWindow` rendering the `/overlay`
/// Svelte route (`src/lib/Overlay/Overlay.svelte`) — the same component tree
/// that renders every built-in visualizer style, the user's custom overlays,
/// and the speaking/command/MCP pills, already wired to the app-wide
/// `status-tick` / `audio-level` events.
///
/// `anchor` / `monitor_pref` are `config.ui.overlay_position` /
/// `overlay_monitor`.
/// How long the overlay may stay hidden waiting for its content to paint.
///
/// The frontend normally reports in well inside this (see `reveal_overlay`),
/// so this only matters when it cannot — a custom overlay whose script throws
/// before the report, say, or a first paint that is simply slow.
///
/// This used to be 600ms, on the reasoning that the worst case was "a prompt
/// overlay rather than a missing one". That reasoning assumed a slow paint
/// still eventually paints *something* sane. Reported on Hyprland/XWayland
/// with an older Intel iGPU (#134): the overlay this call pre-builds at
/// startup — with nothing to show, since no dictation is running — came up
/// and stayed on screen indefinitely after the timeout fired, rather than
/// clearing once real content (or the lack of it) actually painted. The
/// timeout firing before WebKitGTK's first real paint on a slow GPU path is
/// the most likely explanation: what gets revealed at that point is whatever
/// is sitting in an as-yet-unpainted backing buffer, and unlike a genuinely
/// missing overlay (annoying but momentary — the next tick tries again),
/// nothing ever repaints it back to blank afterwards. A longer window gives
/// a slow first paint more room to actually finish before that risk is
/// taken, at the cost of a slower-appearing overlay only in the case this is
/// meant to catch (which is already the degraded path).
const OVERLAY_REVEAL_TIMEOUT: Duration = Duration::from_millis(3000);

/// Make the overlay window visible, if it is not already.
///
/// A freshly built `WebviewWindow` is mapped long before there is anything to
/// see in it: the webview has to be created, `/overlay` loaded, Svelte
/// mounted and a frame painted, which is tens to hundreds of milliseconds
/// during which the window is on screen with nothing drawn into it. That was
/// showing up as a black box flashing over the overlay's full bounds on every
/// keybind press. It only became visible when the window started being built
/// fresh for each dictation (see `tray.rs`'s `spawn_status_ticker`); built
/// once at startup it happened a single time, before anyone was watching.
///
/// So the window is created fully transparent (`set_opacity(0.0)` in
/// `open_overlay_window`) and revealed here, once the frontend reports that it
/// has painted. Opacity rather than staying unmapped or parking it offscreen:
/// an unmapped webview may not render at all, so waiting on a paint that never
/// comes would hang, and an offscreen position can be clamped by the window
/// manager.
///
/// Be clear about what this did and did not achieve, so it is not mistaken
/// for the fix: on the system where the black box was reported (KDE/KWin,
/// XWayland) it made no observable difference. What removed it from every
/// activation was building the window once instead of per dictation, and what
/// moved the remaining one out of sight was building it during startup. This
/// is kept because not showing a window that has nothing painted in it is
/// right regardless, and it is contained and safe: a mapped, fully
/// transparent window renders exactly as normal, and where there is no
/// compositor `set_opacity` does nothing and behaviour is simply unchanged.
pub fn reveal_overlay(app: &tauri::AppHandle) {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW) else {
        return;
    };
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        if let Ok(gtk_window) = window.gtk_window() {
            gtk_window.set_opacity(1.0);
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = window;
}

pub fn open_overlay_window(
    app: &tauri::AppHandle,
    anchor: &str,
    monitor_pref: &str,
) -> Result<tauri::WebviewWindow, String> {
    // Only a window built by this call starts hidden. Re-entering with one
    // already on screen must not blank the overlay the user is looking at.
    let is_new = app.get_webview_window(OVERLAY_WINDOW).is_none();

    let window = match app.get_webview_window(OVERLAY_WINDOW) {
        Some(existing) => existing,
        None => tauri::WebviewWindowBuilder::new(
            app,
            OVERLAY_WINDOW,
            tauri::WebviewUrl::App("/overlay".into()),
        )
        .title("VoxCtrl Overlay")
        .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .closable(false)
        .minimizable(false)
        .maximizable(false)
        .visible(false)
        .build()
        .map_err(|e| format!("Could not create the overlay window: {e}"))?,
    };

    // Everything the window manager reads when deciding how to present a
    // window has to be set BEFORE the window is mapped. Applied afterwards,
    // the window is mapped as a default-type window at a position of the
    // WM's choosing, and the WM then re-evaluates it a moment later — which
    // on KWin draws a brief black frame around the window's full bounds on
    // every activation. `realize()` creates the underlying GdkWindow without
    // mapping it, which is what gives us somewhere to put these first.
    //
    // This is only visible because the overlay window is now built fresh for
    // each dictation and destroyed when it goes idle (see `tray.rs`'s
    // `spawn_status_ticker`). Built once at startup, as it used to be, the
    // same re-evaluation happened a single time where nobody was looking.
    //
    // The type hint is `Utility` rather than `Notification`: the app is
    // forced through XWayland on Linux (see `lib.rs`'s `GDK_BACKEND=x11`
    // override — this window's absolute positioning and always-on-top depend
    // on it), and KWin's X11 compositing path gives
    // `_NET_WM_WINDOW_TYPE_NOTIFICATION` windows different,
    // short-lived-oriented repaint handling. Reported symptom that was meant
    // to fix: every overlay style, on every keybind release, freezes solid on
    // screen — sometimes clearing when another application is launched, never
    // on its own, only fixed by quitting the app entirely — despite the
    // overlay's own content genuinely finishing its unmount (confirmed: the
    // next activation correctly animates in over the stuck frame). `Utility`
    // is KWin's ordinary type for a persistent always-on-top panel and goes
    // through the normal compositing/repaint path, while still being excluded
    // from alt-tab in every WM this has been checked against.
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        if let Ok(gtk_window) = window.gtk_window() {
            gtk_window.realize();
            if let Some(gdk_window) = gtk_window.window() {
                gdk_window.set_type_hint(gtk::gdk::WindowTypeHint::Utility);
            }
            // Mapped but fully transparent until its content has painted —
            // see `reveal_overlay`. Set before `show()` so there is no frame
            // in which the window is both mapped and opaque.
            if is_new {
                gtk_window.set_opacity(0.0);
            }
        }
    }

    // Likewise the position: placed before mapping, the window appears where
    // it belongs instead of being put somewhere by the WM and moved after the
    // fact. It is applied again below, since a WM is free to place a window
    // where it likes regardless of what was asked for before the map.
    reposition_overlay_inner(&window, anchor, monitor_pref);

    // On Linux, tao's `set_ignore_cursor_events` reaches into the GTK
    // window's underlying GdkWindow and unwraps it unconditionally
    // (tao/src/platform_impl/linux/event_loop.rs, WindowRequest::CursorIgnoreEvents).
    // That GdkWindow does not exist until the widget is realized, so it has
    // to come after the block above (which realizes it explicitly) or after
    // `show()` (which realizes it as a side effect). Calling it any earlier
    // panics — and, being inside a GTK callback, aborts the whole process
    // instead of unwinding.
    if let Err(e) = window.show() {
        tracing::error!("Failed to show overlay window: {:?}", e);
    }

    // Click-through: mouse events pass to whatever is beneath the overlay.
    if let Err(e) = window.set_ignore_cursor_events(true) {
        tracing::warn!("Failed to make overlay window click-through: {:?}", e);
    }

    reposition_overlay_inner(&window, anchor, monitor_pref);

    // Safety net for the hidden-until-painted gate above: if the frontend
    // never reports in, reveal it anyway. An overlay that flashes black is a
    // blemish; one that never appears is a broken feature, and a custom
    // overlay is arbitrary user-supplied HTML that may simply fail.
    if is_new {
        let handle = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(OVERLAY_REVEAL_TIMEOUT).await;
            reveal_overlay(&handle);
        });
    }

    Ok(window)
}

/// Poll interval for [`suspend_overlay_without_outputs`]. Far coarser than
/// the ticker's own 150ms cadence: this only ever needs to react to a screen
/// going idle/locked, never to anything on the hot dictation path.
#[cfg(target_os = "linux")]
pub const NO_OUTPUTS_POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Hide the overlay window while the compositor reports no real outputs, and
/// show it again once one comes back.
///
/// The bundled WebKitGTK paces its compositor off a `DisplayVBlankMonitor`
/// thread that divides by the current output's refresh rate. When a
/// compositor (seen on Hyprland) drops to zero real outputs — idle, screen
/// lock, DPMS off — and falls back to a placeholder monitor, that monitor has
/// been observed reporting a scale of 0 to GTK
/// (`gdk_monitor_set_scale: assertion 'scale > 0.' failed`); a refresh rate of
/// 0 read into the same divide crashes the whole process with `SIGFPE`. Other
/// GTK apps survive the assertion; VoxCtrl's bundled WebKitGTK does not
/// survive the divide.
///
/// The overlay window is built once at startup and kept mapped for the rest
/// of the session (see `open_overlay_window`'s doc comment for why it is not
/// rebuilt per dictation), so whatever thread paces its compositor normally
/// keeps running for as long as VoxCtrl does — including through an idle
/// desktop, which is exactly when a compositor is most likely to drop to zero
/// outputs. Unmapping the window for as long as there is nothing to display
/// it on anyway is the one lever available without patching a stripped,
/// bundled library: an unmapped `WebviewWindow` is not something WebKitGTK
/// paces frames for. This narrows the crash window to the poll interval
/// above rather than closing it, and it does nothing at all once WebKitGTK
/// (bundled or the host's own — see
/// `scripts/appimage-hooks/host-first-fallback.sh`) has the underlying
/// divide-by-zero fixed upstream.
#[cfg(target_os = "linux")]
pub fn suspend_overlay_without_outputs(app: &tauri::AppHandle, suspended: &mut bool) {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW) else {
        return;
    };
    // Any error is treated as "outputs present": this must never hide the
    // overlay on a desktop that is working normally just because a query
    // failed.
    let has_outputs = window.available_monitors().map(|m| !m.is_empty()).unwrap_or(true);

    if !has_outputs && !*suspended {
        *suspended = true;
        tracing::info!(
            "No display outputs reported; hiding the overlay window until one returns \
             (works around a SIGFPE in the bundled WebKitGTK's vblank thread)"
        );
        let _ = window.hide();
    } else if has_outputs && *suspended {
        *suspended = false;
        let _ = window.show();
    }
}

/// Re-apply the anchor/monitor position to the overlay window, if it
/// currently exists.
///
/// Called whenever `config.ui.overlay_position` / `overlay_monitor` change
/// (see `commands.rs`'s `save_config` and `tray.rs`'s config-change ticker,
/// both of which send a `{"type":"position","position":..,"monitor":..}`
/// message over `overlay_tx` — see its consumer in `lib.rs`), so
/// position/monitor changes take effect live instead of only at startup.
pub fn reposition_overlay(app: &tauri::AppHandle, anchor: &str, monitor_pref: &str) {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW) {
        reposition_overlay_inner(&window, anchor, monitor_pref);
    }
}

fn reposition_overlay_inner(window: &tauri::WebviewWindow, anchor: &str, monitor_pref: &str) {
    if let Some((x, y)) = compute_overlay_window_position(window, anchor, monitor_pref) {
        let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
    }
}

/// Re-assert the overlay window's always-on-top state, if it exists.
///
/// The window level is not "sticky": another window taking `_NET_WM_STATE_ABOVE`,
/// a fullscreen app, or the compositor re-stacking between dictations can push
/// the overlay behind other windows, and it never recovers on its own.
/// Toggling the level off then back on (rather than setting `true` when it
/// may already be `true`) forces the change to actually reach the window
/// manager rather than being suppressed as a no-op.
///
/// A no-op when the overlay window doesn't exist yet, so callers on the hot
/// audio-level path can call this unconditionally.
pub fn reassert_overlay_topmost(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW) {
        let _ = window.set_always_on_top(false);
        let _ = window.set_always_on_top(true);
    }
}

/// Top-left Y for the overlay given the anchor, in the same pixel space as
/// the monitor geometry.
fn overlay_anchor_y(monitor_y: i32, monitor_height: i32, window_height: i32, margin: i32, anchor: &str) -> i32 {
    match anchor {
        "top" => monitor_y + margin,
        "bottom" => monitor_y + monitor_height - window_height - margin,
        _ => monitor_y + (monitor_height - window_height) / 2, // "center" default
    }
}

/// Compute the overlay's top-left pixel position for an anchor, using the
/// window's own monitor + size.
fn compute_overlay_window_position(
    window: &tauri::WebviewWindow,
    anchor: &str,
    monitor_pref: &str,
) -> Option<(i32, i32)> {
    let monitors = window.available_monitors().ok()?;
    let monitor = if monitor_pref.is_empty() || monitor_pref == "primary" {
        window.primary_monitor().ok().flatten().or_else(|| monitors.first().cloned())
    } else {
        monitors
            .iter()
            .find(|m| m.name().map(|n| n.as_str()) == Some(monitor_pref))
            .cloned()
            .or_else(|| window.primary_monitor().ok().flatten())
            .or_else(|| monitors.first().cloned())
    }?;

    let msize = monitor.size();
    let mpos = monitor.position();
    let scale = monitor.scale_factor();
    let margin = (60.0 * scale) as i32;

    // Physical-pixel size of the overlay, computed from the target monitor's
    // scale factor rather than queried via `window.outer_size()`: called this
    // soon after `show()`, outer_size() can still report 0x0 because the
    // resize hasn't round-tripped through the X11/GTK event loop yet. That
    // silently produced `None` here every time, leaving the window wherever
    // the window manager defaults new windows to (its own primary-monitor
    // center) — which is why position/monitor settings appeared to be
    // ignored entirely. OVERLAY_WIDTH/OVERLAY_HEIGHT are fixed (the window is
    // non-resizable), so there's no need to ask the window for its size at all.
    let wwidth = (OVERLAY_WIDTH * scale) as i32;
    let wheight = (OVERLAY_HEIGHT * scale) as i32;

    let x = mpos.x + (msize.width as i32 - wwidth) / 2;
    let y = overlay_anchor_y(mpos.y, msize.height as i32, wheight, margin, anchor);
    Some((x, y))
}

/// Show the update offer, building its window if it is not already there.
///
/// This is the one window that appears without the user having asked for
/// anything, so it is a plain window rather than an always-on-top one: an
/// update is worth mentioning, not worth interrupting whatever is on screen.
/// VoxCtrl is a tray app that usually has nothing open at all, which is why the
/// news cannot simply go into Settings.
pub fn open_update_window(app: &tauri::AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(UPDATE_WINDOW) {
        show_and_focus_window(&existing);
        return Ok(());
    }

    let window = tauri::WebviewWindowBuilder::new(
        app,
        UPDATE_WINDOW,
        tauri::WebviewUrl::App("/update".into()),
    )
    .title("VoxCtrl — Update Available")
    .inner_size(560.0, 620.0)
    .min_inner_size(460.0, 420.0)
    .center()
    .resizable(true)
    .decorations(true)
    .build()
    .map_err(|e| format!("Could not open the update window: {e}"))?;

    show_and_focus_window(&window);
    Ok(())
}

/// Bring the setup window to the front, building it if it has been closed.
pub fn show_setup_window() {
    let Some(handle) = APP_HANDLE.get() else {
        return;
    };
    if let Some(w) = handle.get_webview_window(SETUP_WINDOW) {
        raise_window(&w, true);
        return;
    }

    let built = tauri::WebviewWindowBuilder::new(
        handle,
        SETUP_WINDOW,
        tauri::WebviewUrl::App("/udev-warning".into()),
    )
    .title("VoxCtrl Setup")
    .inner_size(580.0, 600.0)
    .min_inner_size(480.0, 420.0)
    .center()
    .always_on_top(true)
    .resizable(true)
    .build();
    if let Err(e) = built {
        tracing::error!("Could not open the setup window: {e}");
    }
}

/// What is stopping dictation from working end to end, as a message to show the
/// user. `None` means the app is fully set up.
///
/// Called on every hotkey activation: pressing the shortcut and getting silence
/// is the moment the user needs to be told the install is unfinished, and it is
/// the only moment we know for certain they are trying to dictate.
pub async fn setup_blocker(state: &Arc<AppState>) -> Option<String> {
    if let Some(tool) = commands::missing_injection_tool() {
        return Some(format!(
            "VoxCtrl cannot type text into other windows: '{tool}' is not installed. \
             Finish the setup in VoxCtrl → Settings to install it."
        ));
    }

    let cfg = state.config.lock().await;
    let eng = &cfg.data.engine;
    // Moonshine or Parakeet only bypasses the Whisper-model check when it is actually
    // compiled in; otherwise the app silently falls back to whisper-cpp and
    // still needs the model.
    let uses_whisper_model = eng.backend != voxctrl_config::BackendChoice::RemoteOpenAi
        && (eng.backend != voxctrl_config::BackendChoice::Moonshine
            || !voxctrl_inference::MOONSHINE_COMPILED)
        && (eng.backend != voxctrl_config::BackendChoice::Parakeet
            || !voxctrl_inference::PARAKEET_COMPILED);
    if !uses_whisper_model {
        return None;
    }
    // Skip small models the startup hook auto-downloads in the background —
    // that flow has its own "downloading…/ready/failed" notifications, so this
    // would only add a confusing "go to Settings" message mid-download.
    if voxctrl_inference::whisper_cpp::is_small_auto_downloadable(&eng.whisper_cpp.model_size) {
        return None;
    }
    if voxctrl_inference::whisper_cpp::is_model_downloaded(
        &eng.whisper_cpp.model_size,
        &eng.whisper_cpp.model_dir,
    ) {
        return None;
    }

    Some(format!(
        "Speech model '{}' is not downloaded — dictation cannot produce text. \
         Open Settings → Engine and download it.",
        eng.whisper_cpp.model_size
    ))
}

/// Helper to robustly show, unminimize, and focus a window
pub fn show_and_focus_window(window: &tauri::WebviewWindow) {
    raise_window(window, false);
}

/// Bring a window to the front and give it keyboard focus.
///
/// `set_focus` on its own is not enough on Linux. Every mainstream desktop
/// implements focus-stealing prevention: a process that does not already own
/// the focused window has its focus requests demoted to "urgent" — the taskbar
/// entry blinks and the window stays exactly where it was, behind whatever the
/// user is looking at. A tray click is precisely that case, since the tray is
/// not the app's own window.
///
/// Pinning the window above others is honoured where a bare focus request is
/// not, so the raise is done by pinning, focusing, then unpinning a moment
/// later — long enough for the compositor to restack, short enough that the
/// window does not linger above everything else.
///
/// `keep_on_top` leaves the pin in place, for windows that are meant to stay
/// above other applications.
pub fn raise_window(window: &tauri::WebviewWindow, keep_on_top: bool) {
    // Deliberately no `center()`: this is also the path for a window that is
    // already open, and moving a window the user has placed is not what
    // "bring it to the front" means.
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();

    if !keep_on_top {
        let w = window.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let _ = w.set_always_on_top(false);
        });
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 1080p at 100% scaling is the common case: wide enough for a comfortable
    /// stage, and short enough that the height lands exactly on the floor.
    #[test]
    fn a_1080p_display_gets_a_usable_stage() {
        let (w, h) = wizard_size_for(1920.0 * WIZARD_FIT_W, 1080.0 * WIZARD_FIT_H);
        assert!((WIZARD_MIN_WIDTH..=WIZARD_WIDTH).contains(&w), "width {w}");
        assert!((WIZARD_MIN_HEIGHT..=WIZARD_HEIGHT).contains(&h), "height {h}");
        assert!(h <= 1080.0, "a {h}px window does not fit a 1080p display");
    }

    /// 1080p at 125% — the desktop is only 1536x864 logical pixels, which is
    /// under the layout's floor. The floor wins: a window the user has to move
    /// beats a layout that has wrapped.
    #[test]
    fn a_scaled_1080p_display_gets_at_least_the_layout_minimum() {
        let (avail_w, avail_h) = (1536.0 * WIZARD_FIT_W, 864.0 * WIZARD_FIT_H);
        let (w, h) = wizard_size_for(avail_w, avail_h);
        assert!(w >= WIZARD_MIN_WIDTH && h >= WIZARD_MIN_HEIGHT);
    }

    #[test]
    fn the_window_keeps_its_proportions_when_it_shrinks() {
        // Wide enough to be capped by the design width, tall enough that the
        // height floor does not kick in.
        let (w, h) = wizard_size_for(2000.0, 3000.0);
        let aspect = WIZARD_WIDTH / WIZARD_HEIGHT;
        assert!(
            ((w / h) - aspect).abs() < 0.01,
            "expected {aspect}:1, got {w}x{h}"
        );
    }

    /// A display too small for the layout gets the minimum rather than a
    /// window whose contents wrap: the user can move a window, but cannot
    /// unwrap a layout.
    #[test]
    fn a_small_display_never_goes_below_the_layout_minimum() {
        let (w, h) = wizard_size_for(900.0, 500.0);
        assert_eq!((w, h), (WIZARD_MIN_WIDTH, WIZARD_MIN_HEIGHT));
    }

    /// The breakpoint the CSS actually cares about: every wizard step is
    /// designed for a stage wider than its widest `max-width` media query, and
    /// tall enough not to push the footer past the bottom edge.
    #[test]
    fn every_display_clears_the_widest_css_breakpoint() {
        for (avail_w, avail_h) in [
            (1920.0, 1080.0),
            (1536.0, 864.0),
            (1280.0, 720.0),
            (3840.0, 2160.0),
            (900.0, 500.0),
        ] {
            let (w, h) = wizard_size_for(avail_w * WIZARD_FIT_W, avail_h * WIZARD_FIT_H);
            assert!(w >= WIZARD_MIN_WIDTH, "{avail_w}x{avail_h} gave a {w}px-wide window");
            assert!(h >= WIZARD_MIN_HEIGHT, "{avail_w}x{avail_h} gave a {h}px-tall window");
        }
    }

    #[test]
    fn a_large_display_is_capped_at_the_design_size() {
        let (w, h) = wizard_size_for(3840.0, 2160.0);
        assert_eq!((w, h), (WIZARD_WIDTH, WIZARD_HEIGHT));
    }
}
