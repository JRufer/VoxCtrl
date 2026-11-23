# GPUI frontend evaluation (spike)

**Status:** research only. Nothing in the shipping app has changed on this
branch (`spike/gpui-frontend`). This document records what replacing the
Tauri/Svelte frontend with GPUI would involve, so the spike can be scoped
before any code is written.

## What the frontend is today

| Surface | Where | Size |
|---|---|---|
| Settings (11 tabs, modals) | `src/lib/Settings/` | ~10,400 lines |
| Setup wizard | `src/lib/Wizard/` | ~7,000 lines |
| Overlay (8 built-in styles, live text) | `src/lib/Overlay/` | ~3,200 lines |
| History window | `src/lib/History/` | ~700 lines |
| Update dialog, Setup/Diagnostics | `src/lib/Update/`, `src/lib/Diagnostics/` | ~900 lines |
| Tauri shell (windows, tray, commands) | `src-tauri/src/` | ~12,800 lines, 110 `#[tauri::command]`s |

About 23,000 lines of Svelte/TypeScript/CSS in total. The engine crates under
`crates/` have no Tauri dependency, so they would carry over untouched; the
port is `src/` plus the Tauri-facing half of `src-tauri/`.

## What GPUI is, as of November 2026

- Zed's GPU-accelerated Rust UI framework: a flexbox layout with a
  Tailwind-style builder API, drawn directly on the GPU. Apache-2.0, pre-1.0,
  breaking changes between releases.
- Linux rendering moved from Blade to `wgpu` in February 2026, which closed a
  set of NVIDIA and Wayland freezes.
- It has a native Wayland backend with `wlr-layer-shell` support
  (`WindowKind::LayerShell`): anchored, always-on-top, non-focusable surfaces
  placed by the compositor.
- The `gpui` crate on crates.io (0.2.2) is stale. In practice a standalone app
  depends on GPUI Kit (`gpui-kit` 0.11, formerly `gpui-component`), which pins
  a republished GPUI snapshot (`gpui-pre` 0.3.14) and adds 75+ styled
  components: inputs, selects, sliders, switches, tables, virtual lists,
  charts, themes, Markdown rendering.
- Not included: a system tray, a webview, HTML/CSS rendering, file dialogs via
  a plugin system, an auto-updater, or a bundler.

## Benefits

1. **The overlay becomes a real Wayland surface.** Today the whole app is
   forced onto XWayland (`GDK_BACKEND=x11` in `lib.rs`) because Wayland gives
   a normal window no way to position itself or stay on top. A layer-shell
   surface is positioned and stacked by the compositor, so the once-a-second
   `reassert_overlay_topmost` heartbeat, the focus-hint workarounds and the
   XWayland requirement would all go.
2. **WebKitGTK goes away.** `render_env.rs`, the AppImage host-first
   libwebkit2gtk fallback, the NVIDIA DMA-BUF handling, the black-box-on-map
   and missing-alpha bugs, and the `libwebkit2gtk-4.1-0` package dependency
   all exist only because of the webview.
3. **Visual headroom.** Overlays would animate at display refresh rate on the
   GPU with per-frame access to the audio level, instead of CSS driven by
   custom properties. Custom-painted waveforms, spectra, gradients, blurs and
   spring animation are direct draw calls.
4. **One language, no IPC.** The 110 commands and the event bus
   (`status-tick`, `audio-level`, `config-changed`, ...) become plain function
   calls and shared state. Node, Vite, Svelte and Tailwind leave the build.
5. **Lower latency to first paint.** No webview to load before the overlay can
   show.

## Trade-offs

1. **Custom overlays break.** A custom overlay is an `index.html` plus
   `style.css` injected into the webview. GPUI cannot render HTML or CSS.
   Every overlay in `VoxCtrl-Scripts-and-Overlays`, the `Custom/` example,
   the overlay README and both overlay-designer skills depend on that format.
   The options are: (a) a new declarative overlay format (for example TOML or
   RON describing shapes bound to the same live values), (b) keep a webview
   only for custom overlays, which keeps WebKitGTK and most of its problems,
   or (c) drop user-authored overlays. This is the decision that most shapes
   the project and needs to be made before overlay work starts.
2. **Layer shell is not universal.** KDE Plasma, Hyprland, Sway, niri and
   COSMIC implement it. GNOME (Mutter) does not, and X11 sessions have no such
   protocol. Those need a second path: an X11 override-redirect window, or
   XWayland on GNOME as today.
3. **It is a rewrite, not a port.** Roughly 23,000 lines of UI have no
   mechanical translation. The key recorder, target editor, TTS tab and
   wizard are the expensive parts.
4. **Missing platform pieces must be supplied.** Tray (`ksni` on Linux, a
   different crate on Windows/macOS), single-instance, notifications
   (`notify-rust` is already a workspace dependency), file dialogs, and
   packaging (`cargo-bundle`/`cargo-packager`, plus a reworked AppImage
   script and Windows installer).
5. **Windows and macOS.** GPUI supports both, but the Windows installer flow
   and the overlay's click-through/topmost behaviour would need re-proving
   there.
6. **Dependency risk.** Pre-1.0 API, and the practical distribution is a
   third-party republication of Zed's crate. The dependency tree is large
   (about 1,400 resolved lines for `gpui-kit` alone) and builds are slow.
7. **Accessibility and text input.** Screen-reader support and IME handling
   are less mature than WebKit's.
8. **GPU required.** `wgpu` needs working Vulkan or GL; software fallback is
   much weaker than a webview's.
9. **Contributor reach.** Fewer people can restyle a GPUI view than can edit
   CSS.

## Recommended shape for the spike

Additive, in a new workspace crate, leaving `src/` and `src-tauri/` alone:

1. **Overlay first.** One layer-shell overlay style with a live audio-level
   visualiser and live text, fed by the existing pipeline. This tests the
   largest benefit and exposes the custom-overlay question in practice.
2. **One Settings tab** (Audio: device select, sliders, live meter) on GPUI
   Kit, to measure the cost per tab.
3. **Tray** via `ksni`.

A viable middle path if the full rewrite is not worth it: keep
Settings/Wizard/History in Tauri and move only the overlay to a small GPUI
process. That captures benefits 1, 3 and 5 for a fraction of the cost.

## Open questions

- Which of the three custom-overlay options is acceptable?
- Is a GNOME/X11 fallback path acceptable for the overlay?
- Full replacement, or overlay-only?
