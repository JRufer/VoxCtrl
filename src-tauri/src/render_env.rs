//! Graphics-stack environment defaults for the Linux build.
//!
//! GTK, GDK and WebKit each read this environment once, when they initialise,
//! so everything here has to be in place before any of them start — which is
//! why `main()` applies it before anything else runs.
//!
//! Nothing here is forced. Every value is a *default*: a variable already set
//! in the environment is left alone. That matters more than it looks. These
//! knobs are reachable no other way, and while the overlay's closing-animation
//! smear was being tracked down, each one that had to be guessed at cost a
//! full build-and-release cycle to test, because the app overwrote whatever
//! the shell set. Being able to run
//!
//! ```text
//! WEBKIT_DISABLE_DMABUF_RENDERER=0 ./VoxCtrl-....AppImage
//! ```
//!
//! and get the behaviour that asks for is worth keeping. (It is also how the
//! WebKitGTK 2.54 fix below was found and confirmed on an affected machine.)
//!
//! ## Which renderer, by WebKitGTK version
//!
//! On the proprietary NVIDIA driver, WebKit's default DMA-BUF renderer can't
//! create its GPU buffers ("Failed to create GBM buffer"), and a transparent
//! window comes up solid black or empty. VoxCtrl has always avoided that.
//!
//! - **Before 2.54:** `WEBKIT_DISABLE_DMABUF_RENDERER=1`, which switches WebKit
//!   to its simpler, non-accelerated renderer. That worked everywhere.
//! - **2.54 and newer:** that same renderer now repaints only the regions that
//!   changed each frame, and in a transparent window it wipes everything else
//!   to fully transparent. The overlay showed only its constantly animated
//!   parts, everything static vanished (reported on KDE Plasma 6.8, NVIDIA
//!   615.78, WebKitGTK 2.54.2; the same regression hit other WebKitGTK apps,
//!   and the upstream fix was closed unmerged). So on 2.54+ VoxCtrl keeps the
//!   accelerated DMA-BUF renderer, which composites transparency correctly,
//!   and sets `WEBKIT_DMABUF_RENDERER_FORCE_SHM=1` so its frames reach the
//!   window through shared memory instead of GPU buffers. NVIDIA never has to
//!   allocate one, and the overlay draws whole.
//!
//! On the smear itself: it was WebKitGTK, but not through anything settable
//! here. The AppImage bundled ubuntu-22.04's WebKitGTK, which renders a
//! transparent window's compositing layers without their alpha channel. The
//! fix is in the packaging — `usr/lib/fallback` now holds libwebkit2gtk so the
//! host's copy is preferred (see `scripts/appimage-hooks/host-first-fallback.sh`).
//! Two environment workarounds tried before that was understood have since
//! been removed: `WEBKIT_DISABLE_COMPOSITING_MODE`, a no-op on WebKitGTK
//! releases that dropped the legacy software path, and `LIBGL_ALWAYS_SOFTWARE`,
//! which did nothing for the smear (software rasterisation smeared
//! identically) while making the webview slow enough to paint that a freshly
//! created overlay window showed its unpainted black backing for a few frames
//! first.

/// The first WebKitGTK release whose non-accelerated renderer wipes the
/// undamaged parts of a transparent window.
pub const PARTIAL_REPAINT_WEBKIT: (u32, u32) = (2, 54);

/// The two variables that choose WebKit's renderer here. A user who sets
/// either has picked a renderer themselves, so neither default applies.
const RENDERER_VARS: [&str; 2] = ["WEBKIT_DISABLE_DMABUF_RENDERER", "WEBKIT_DMABUF_RENDERER_FORCE_SHM"];

/// Environment defaults for the graphics stack, as `(key, value)`.
///
/// Pure, so the policy is testable without mutating the process environment:
/// `is_set` reports whether a variable already has a value (anything it
/// reports as set is left alone), and `webkit` is the running WebKitGTK's
/// `(major, minor)` version, `None` when it can't be read.
pub fn rendering_defaults(
    is_set: impl Fn(&str) -> bool,
    webkit: Option<(u32, u32)>,
) -> Vec<(&'static str, &'static str)> {
    if RENDERER_VARS.iter().any(|key| is_set(key)) {
        return Vec::new();
    }
    match webkit {
        // Keep the accelerated renderer, which handles transparency, and
        // hand its frames over through shared memory so NVIDIA never has to
        // allocate a GPU buffer for them.
        Some(version) if version >= PARTIAL_REPAINT_WEBKIT => {
            vec![("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1")]
        }
        // Older WebKitGTK, or a version that couldn't be read: the simple
        // renderer, which on these releases repaints whole frames.
        _ => vec![("WEBKIT_DISABLE_DMABUF_RENDERER", "1")],
    }
}

/// The running WebKitGTK's `(major, minor)` version.
///
/// Asked of the loaded library, not taken from build time: the AppImage
/// prefers the host's WebKitGTK, so the version that matters is whichever one
/// this process actually runs.
#[cfg(target_os = "linux")]
pub fn webkit_version() -> Option<(u32, u32)> {
    extern "C" {
        fn webkit_get_major_version() -> std::ffi::c_uint;
        fn webkit_get_minor_version() -> std::ffi::c_uint;
    }
    // SAFETY: both return constants compiled into the linked libwebkit2gtk;
    // they take no arguments and need no initialisation.
    let (major, minor) = unsafe { (webkit_get_major_version(), webkit_get_minor_version()) };
    (major > 0).then_some((major, minor))
}

/// Apply [`rendering_defaults`] to this process.
#[cfg(target_os = "linux")]
pub fn apply() {
    let webkit = webkit_version();
    for (key, value) in rendering_defaults(|key| std::env::var_os(key).is_some(), webkit) {
        std::env::set_var(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEFORE: Option<(u32, u32)> = Some((2, 52));
    const AFFECTED: Option<(u32, u32)> = Some((2, 54));

    #[test]
    fn before_2_54_the_simple_renderer_is_used() {
        let vars = rendering_defaults(|_| false, BEFORE);
        assert_eq!(vars, vec![("WEBKIT_DISABLE_DMABUF_RENDERER", "1")]);
    }

    /// The overlay lost every static part on 2.54 with the simple renderer.
    #[test]
    fn from_2_54_the_accelerated_renderer_shares_frames_through_memory() {
        for version in [AFFECTED, Some((2, 56)), Some((3, 0))] {
            let vars = rendering_defaults(|_| false, version);
            assert_eq!(vars, vec![("WEBKIT_DMABUF_RENDERER_FORCE_SHM", "1")], "{version:?}");
            assert!(!vars.iter().any(|(k, _)| *k == "WEBKIT_DISABLE_DMABUF_RENDERER"));
        }
    }

    #[test]
    fn an_unknown_version_keeps_the_long_standing_default() {
        let vars = rendering_defaults(|_| false, None);
        assert_eq!(vars, vec![("WEBKIT_DISABLE_DMABUF_RENDERER", "1")]);
    }

    /// The point of these being defaults: someone debugging their own machine
    /// can override them from a shell, with no rebuild.
    #[test]
    fn a_renderer_the_user_chose_is_never_overridden() {
        for key in RENDERER_VARS {
            for version in [BEFORE, AFFECTED, None] {
                assert!(rendering_defaults(|k| k == key, version).is_empty(), "{key} {version:?}");
            }
        }
    }

    /// Software rasterisation was briefly forced inside the AppImage while the
    /// smear's cause was still unknown. It never fixed it, and it delayed the
    /// overlay window's first paint enough to show a black box. It should not
    /// come back without evidence it is needed.
    #[test]
    fn software_rasterisation_is_not_forced() {
        for version in [BEFORE, AFFECTED, None] {
            let vars = rendering_defaults(|_| false, version);
            assert!(!vars.iter().any(|(key, _)| *key == "LIBGL_ALWAYS_SOFTWARE"), "{vars:?}");
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_running_webkit_version_can_be_read() {
        let (major, _) = webkit_version().expect("libwebkit2gtk is linked");
        assert!(major >= 2);
    }
}
