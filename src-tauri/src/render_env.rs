//! Rendering-stack environment for the Linux build.
//!
//! The AppImage ships a *split* graphics stack, and the overlay's alpha
//! blending is exactly where the split shows.
//!
//! `build_appimage.sh` and `.github/workflows/release.yml` both delete
//! `libEGL`/`libGL`/`libGLX`/`libglapi`/`libgbm`/`libdrm` from the bundle so
//! the app falls through to the host's Mesa — without that, an AppImage built
//! on one distro renders a frame and then never gets another frame callback on
//! a newer one. What they deliberately *keep* bundled is `libwebkit2gtk`
//! (its helper processes have to sit at an exact relative path, so it cannot
//! be host-first the way the graphics libraries are) and, with it, GTK3/GDK3
//! and Cairo.
//!
//! So a released AppImage runs ubuntu-22.04's WebKitGTK and GDK against
//! whatever Mesa the user's desktop has. Those two halves have to agree on a
//! framebuffer config for exactly one thing: the GL surface WebKit composites
//! a promoted layer into. Everything static is painted through the bundled,
//! self-consistent GDK/Cairo path and gets its alpha right; the moment a CSS
//! transition promotes the overlay to its own composited layer, rendering
//! moves to a GL config negotiated across the seam — and when that config
//! comes back without a usable alpha channel, the overlay's closing animation
//! turns into an opaque, blurry smear of its last frame that sits there until
//! the window is destroyed.
//!
//! That is why the symptom only ever appeared on release builds: a local
//! `build_appimage.sh` bundles the developer's own WebKitGTK/GDK, built
//! against the same Mesa generation as the machine running it, so both halves
//! already agree and there is nothing to mismatch.
//!
//! The fix is to stop straddling the seam. Inside an AppImage the webview
//! composites through Mesa's software rasterizer, which is bundled-stack
//! agnostic: it honours alpha identically on every host, driver and GPU.
//! It costs nothing worth measuring here — the only things WebKit composites
//! are a small always-on-top overlay and the settings window — and it does not
//! touch Vulkan, so GPU speech inference (`libvulkan`, not `libGL`) is
//! unaffected.

/// Environment defaults for the rendering stack, as `(key, value)`.
///
/// Pure so the policy can be tested without mutating the process
/// environment: `appdir` is `$APPDIR` (set by every AppImage's `AppRun`, and
/// absent for a `.deb`, a distro package or `tauri dev`), and `is_set` reports
/// whether a variable already has a value.
///
/// Nothing here is forced. Every value is a *default*: a variable the user
/// already set in their environment is never overwritten, so a bad guess on
/// our part can always be escaped from a shell instead of requiring a new
/// build. That is deliberate — these knobs are not reachable any other way,
/// and each one that had to be guessed blind previously cost a full release
/// cycle to test.
pub fn rendering_defaults(
    appdir: Option<&str>,
    is_set: impl Fn(&str) -> bool,
) -> Vec<(&'static str, &'static str)> {
    let mut vars: Vec<(&'static str, &'static str)> = Vec::new();

    // Pre-existing workaround: without this, a transparent window comes up
    // solid black on the proprietary NVIDIA driver, where DMA-BUF buffer
    // creation fails outright.
    vars.push(("WEBKIT_DISABLE_DMABUF_RENDERER", "1"));

    // The split-stack fix — see this module's documentation. Scoped to
    // AppImage runs by construction: that is the only build that mixes a
    // bundled WebKitGTK/GDK with the host's Mesa. A `.deb`, a distro package
    // or a dev build has one coherent stack and keeps native GL.
    if appdir.is_some_and(|dir| !dir.is_empty()) {
        vars.push(("LIBGL_ALWAYS_SOFTWARE", "1"));
    }

    vars.retain(|(key, _)| !is_set(key));
    vars
}

/// Apply [`rendering_defaults`] to this process.
///
/// Must run before GTK, GDK or WebKit initialise — they read all of this once,
/// at startup — which is why this is called from `main()` before anything else
/// and again at the top of `run()`.
#[cfg(target_os = "linux")]
pub fn apply() {
    let appdir = std::env::var("APPDIR").ok();
    let vars = rendering_defaults(appdir.as_deref(), |key| std::env::var_os(key).is_some());
    for (key, value) in vars {
        std::env::set_var(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nothing_set(_: &str) -> bool {
        false
    }

    #[test]
    fn an_appimage_composites_the_webview_in_software() {
        let vars = rendering_defaults(Some("/tmp/.mount_Vox123"), nothing_set);
        assert!(
            vars.contains(&("LIBGL_ALWAYS_SOFTWARE", "1")),
            "the AppImage is the build that mixes bundled WebKitGTK with host Mesa, \
             which is what breaks alpha on a composited layer: {vars:?}"
        );
    }

    /// A `.deb`, a distro package and `tauri dev` all have one coherent
    /// graphics stack, so there is no seam to work around and no reason to
    /// give up native GL.
    #[test]
    fn a_non_appimage_build_keeps_native_gl() {
        let vars = rendering_defaults(None, nothing_set);
        assert!(!vars.iter().any(|(key, _)| *key == "LIBGL_ALWAYS_SOFTWARE"), "{vars:?}");
    }

    /// An `APPDIR` that is present but empty is not an AppImage.
    #[test]
    fn an_empty_appdir_is_not_an_appimage() {
        let vars = rendering_defaults(Some(""), nothing_set);
        assert!(!vars.iter().any(|(key, _)| *key == "LIBGL_ALWAYS_SOFTWARE"), "{vars:?}");
    }

    /// The NVIDIA black-window workaround is not AppImage-specific: DMA-BUF
    /// creation fails on that driver however the app was installed.
    #[test]
    fn the_dmabuf_workaround_applies_to_every_linux_build() {
        for appdir in [None, Some("/tmp/.mount_Vox123")] {
            let vars = rendering_defaults(appdir, nothing_set);
            assert!(vars.contains(&("WEBKIT_DISABLE_DMABUF_RENDERER", "1")), "{vars:?}");
        }
    }

    /// The whole point of these being defaults: a user debugging their own
    /// machine can override any of them from a shell, without a rebuild.
    #[test]
    fn a_variable_the_user_already_set_is_never_overwritten() {
        let vars = rendering_defaults(Some("/tmp/.mount_Vox123"), |key| {
            key == "LIBGL_ALWAYS_SOFTWARE"
        });
        assert!(!vars.iter().any(|(key, _)| *key == "LIBGL_ALWAYS_SOFTWARE"), "{vars:?}");
        // …and overriding one leaves the rest alone.
        assert!(vars.contains(&("WEBKIT_DISABLE_DMABUF_RENDERER", "1")), "{vars:?}");
    }

    #[test]
    fn overriding_everything_leaves_nothing_to_apply() {
        assert!(rendering_defaults(Some("/tmp/.mount_Vox123"), |_| true).is_empty());
    }
}
