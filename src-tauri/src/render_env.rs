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
//! and get the behaviour that asks for is worth keeping.
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

/// Environment defaults for the graphics stack, as `(key, value)`.
///
/// Pure, so the policy is testable without mutating the process environment:
/// `is_set` reports whether a variable already has a value, and anything it
/// reports as set is dropped rather than overwritten.
pub fn rendering_defaults(is_set: impl Fn(&str) -> bool) -> Vec<(&'static str, &'static str)> {
    let mut vars: Vec<(&'static str, &'static str)> = Vec::new();

    // Without this, a transparent window comes up solid black on the
    // proprietary NVIDIA driver, where DMA-BUF buffer creation fails outright.
    vars.push(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"));

    vars.retain(|(key, _)| !is_set(key));
    vars
}

/// Apply [`rendering_defaults`] to this process.
#[cfg(target_os = "linux")]
pub fn apply() {
    for (key, value) in rendering_defaults(|key| std::env::var_os(key).is_some()) {
        std::env::set_var(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nvidia_black_window_workaround_is_applied_by_default() {
        let vars = rendering_defaults(|_| false);
        assert!(vars.contains(&("WEBKIT_DISABLE_DMABUF_RENDERER", "1")), "{vars:?}");
    }

    /// The point of these being defaults: someone debugging their own machine
    /// can override any of them from a shell, with no rebuild.
    #[test]
    fn a_variable_the_user_already_set_is_never_overwritten() {
        let vars = rendering_defaults(|key| key == "WEBKIT_DISABLE_DMABUF_RENDERER");
        assert!(vars.is_empty(), "{vars:?}");
    }

    /// Software rasterisation was briefly forced inside the AppImage while the
    /// smear's cause was still unknown. It never fixed it, and it delayed the
    /// overlay window's first paint enough to show a black box. It should not
    /// come back without evidence it is needed.
    #[test]
    fn software_rasterisation_is_not_forced() {
        let vars = rendering_defaults(|_| false);
        assert!(!vars.iter().any(|(key, _)| *key == "LIBGL_ALWAYS_SOFTWARE"), "{vars:?}");
    }
}
