# Overlay UI Guide

VoxCtrl displays a visual overlay while the microphone is active, while TTS is speaking, or while the MCP server is actively recording (provided Visual Feedback is enabled). The built-in overlay styles are rendered by an ordinary Tauri **`WebviewWindow`** — the `/overlay` Svelte route (`src/lib/Overlay/Overlay.svelte`) drawn in a borderless, transparent, always-on-top, click-through window (592×222 logical px) over whatever application is in focus. It's the same component tree used for the style preview in Settings, so what you see there is exactly what appears during a real dictation.

---

## How the Overlay Works

`window::open_overlay_window` (`src-tauri/src/window.rs`) builds and configures the overlay window once, at application startup, and it stays mapped for the life of the app — the `/overlay` route just renders nothing visible while idle rather than the window being hidden and remade each time. State reaches it the same way it reaches every other window: the `status-tick` and `audio-level` Tauri events the backend already emits app-wide (see `src/stores/status.ts`), so no separate IPC protocol exists for the overlay.

To avoid focus-stealing and window manager focus grabs during dictation, the window is configured with the following properties:

- **Mapped on Startup** — The window is created and shown immediately on launch, before any dictation happens, so the compositor has already registered, anchored, and placed it by the time it's needed.
- **Non-Focusable & Taskbar Bypassing** — On Linux, the underlying GTK window is given the `WindowTypeHint::Notification` hint. On Windows and macOS the Tauri window builder's own `skip_taskbar` + `focused(false)` options apply. Consequently, the window is excluded from taskbar/app bar listings and does not take keyboard focus or steal focus from the user's cursor.
- **Transparent** — The window background is fully transparent (`transparent(true)`, `shadow(false)`); only the active visualizer elements, rendered as ordinary HTML/CSS/SVG, are visible.
- **Always-on-Top** — Floats persistently above all other active desktop windows. Because a window manager can restack the overlay behind another window mid-session and it never recovers on its own, `window::reassert_overlay_topmost` re-sends the always-on-top state on a ~1s heartbeat while a dictation is active (see `pipeline::spawn_audio_level_forwarder`), not just once at creation.
- **Click-Through** — `set_ignore_cursor_events(true)` disables the window's cursor hit-test at the windowing-system level, so mouse events pass cleanly through to the window beneath it.
- **Borderless / Frameless** — No title bar or decorations.
- **Wayland** — Wayland gives clients no way to position themselves or force always-on-top (that's compositor policy, not a client request), so on a Wayland session with a reachable X server, VoxCtrl forces the whole app onto XWayland at startup (`GDK_BACKEND=x11`, set in `lib.rs` before GTK initializes) to get real positioning and stacking back.

The overlay reveals itself automatically when recording starts and fades out when transcription completes (provided `ui.show_overlay` is enabled). The active style is determined by `config.ui.overlay_style` and is hot-switched without recreating the window. Position updates (`config.ui.overlay_position` / `overlay_monitor`) also apply live — see `window::reposition_overlay` and its caller in `lib.rs`.

### Load & Unload Animations

Every built-in style plays a dedicated load animation when it appears and an unload animation when it disappears, using CSS transitions/keyframes tuned to read as a slightly-underdamped spring (so overlays land with a subtle bounce). The animation is driven client-side in the Svelte component; the window itself **stays mapped throughout** rather than being hidden mid-animation. Each style interprets its own load/unload transition — see the per-style descriptions below.

---

## Screen Positioning

The visual presentation overlay window can be positioned dynamically on the active monitor where your mouse cursor or focused application is located. 

Users can configure the screen alignment under **Settings** -> **Visual Feedback** -> **Overlay position** or manually in `config.json` via `ui.overlay_position`.

Available screen positions:
- **`"center"` (Default)** — Positions the overlay at the exact horizontal and vertical center of the active monitor.
- **`"top"`** — Positions the overlay at the horizontal center, aligned `60` logical pixels from the top of the monitor.
- **`"bottom"`** — Positions the overlay at the horizontal center, aligned `60` logical pixels from the bottom of the monitor.

Tauri dynamically calculates physical pixel values taking into account your display's current high-DPI scaling factor (`scale_factor`), ensuring perfect resolution-independent positioning on 1080p, 1440p, or 4K monitors. Changes to the position setting are instantly applied in real-time if the overlay window is currently visible.

---

## Target Display / Multi-Monitor Support

In multi-monitor setups, VoxCtrl allows you to specify exactly which display screen the visual overlay should appear on.

You can configure the target display under **Settings** -> **Visual Feedback** -> **Overlay display** or manually in `config.json` via `ui.overlay_monitor`.

Options:
- **`"primary"` (Default)** — Constrains the overlay to the OS-defined primary display screen.
- **Specific Monitor Name (e.g. `"HDMI-1"`, `"DP-2"`)** — Connected displays are dynamically queried once at application startup. Selecting one of these binds the visualizer to that specific panel.

### Graceful Disconnection Failover

If the configured target monitor is unplugged or disconnected at runtime, the Tauri backend will automatically fail over to the **Primary Monitor** to keep the visualizer fully accessible. When this happens, a golden warning badge will be displayed inside the settings UI to alert you that the target display is disconnected and fallback mode is active.

---

## Built-in Styles

Eight built-in styles are available — each with its own visual identity, its own kind of audio visualizer, its own load/unload animation, and a clear indicator of the active routing target. The default is **Ocean Wave**.

All styles share three state palettes: **Recording** (the style's signature color), **Initializing** (amber, while the microphone stream is connecting), and **Processing** (sky blue, while the AI is transcribing).

### Ocean Wave *(default — `"blue_wave"`)*
A glass tide pool at night, complete with a glowing moon and rising bubbles.
- **Visualizer**: three layered sine waves (deep blue → cyan → ice teal) whose tide level *and* amplitude rise with your voice. The bottom of the water is always locked to the bottom of the pool — only the waterline moves.
- **Target indicator**: a buoy tag that floats and bobs on the front wave's surface, showing the active target label.
- **Load/unload**: the water fills the pool from the bottom on load and drains back out on unload.
- **States**: "high tide — listening", "low tide — preparing" (initializing), "deep current — processing" (waves shift indigo and surge).

### Voice Card (`"voice_card"`)
A literal membership card: gold contact chip, embossed VOXCTRL branding, and a holographic sheen that drifts across the face.
- **Visualizer**: a 20×6 VU-meter LED dot matrix (green → amber → red, lit bottom-up) with real VU ballistics — instant attack, slow decay — and a centre-weighted envelope.
- **Target indicator**: an embossed `TARGET` field in the card's bottom-left corner, plus a blinking `REC` / `INIT` / `PROC` status stamp top-right.
- **Load/unload**: the card deals in with a flip (horizontal unfold from its centre) and flips back out on unload.

### Waveform (`"waveform"`)
A green-phosphor oscilloscope ("WAVEFORM // OSC-01") with a graticule grid.
- **Visualizer**: a live scrolling line trace of the microphone signal, rendered in two passes (a wide phosphor glow underneath a crisp trace line). Positive samples deflect upward, like a real scope.
- **Target indicator**: a `TGT ▸` readout chip in the scope's top-right corner.
- **Load/unload**: a CRT power-on — the panel expands vertically from a single scanline — and collapses back into the line on unload.
- **States**: "LIVE TRACE" (green), "CALIBRATING" (amber), "TRANSCRIBING" (blue sine sweep on the scope).

### Pulse Ring (`"pulse"`)
A sonar/radar dial paired with a target-lock plate.
- **Visualizer**: a rotating sweep arm with a faded trailing wedge, expanding pulse rings whose brightness tracks your voice, contact blips that flash as the sweep passes their bearing, and an audio-reactive core that swells with the microphone level.
- **Target indicator**: a "PULSE // TARGET LOCK" plate beside the dial with a pulsing reticle (⌖) and lock frame showing the active target label.
- **Load/unload**: the dial drops in and the lock plate slides out from behind it; both reverse on unload.
- **States**: "TARGET LOCK" (tangerine), "ACQUIRING" (amber), "ANALYZING" (blue).

### Mono Bars (`"mono_bars"`)
A hyper-minimal, pure black & white panel — no color, no gradients, no glow.
- **Visualizer**: a 5-bar level meter, centre-weighted so the middle bar reacts most strongly, with a gentle ripple traveling across the row while recording and all bars pulsing in lock-step while processing.
- **Target indicator**: a small caption beneath the bars showing the active target label.
- **Load/unload**: the whole panel simply fades in and out — no motion, scaling, or color, in keeping with its minimal aesthetic.
- **States**: "LISTENING" (dot lit, blinking), "STANDBY" (dim, dot hollow — initializing), "PROCESSING" (bars pulse together, dot blinks faster).

### Neon Spectrum (`"spectrum"`)
A 16-band equalizer in a deep-violet panel with a magenta-to-cyan gradient and a soft glow.
- **Visualizer**: 16 bars across a magenta → purple → cyan gradient. Bass (left) bands swing wider and slower; treble (right) bands flicker faster with a smaller share of the level.
- **Target indicator**: an "OUT ▸" chip in the header showing the active target label.
- **Load/unload**: the panel rises up out of the floor — its height grows while the bars stay anchored to the bottom edge — and sinks back down on unload.
- **States**: "· LIVE" (magenta LED), "· WARMING UP" (amber, initializing), "· ANALYZING" (blue, bars pulse in a traveling wave while processing).

### Retro Terminal (`"terminal"`)
A DOS-blue console window with a three-dot title bar and monospace readouts.
- **Visualizer**: a block-character ASCII meter (`█`/`·`) that fills left-to-right with the audio level.
- **Target indicator**: a `$ voxctrl listen --target "..."` command line showing the active target label.
- **Load/unload**: the console drops down from the top edge on load and retracts back up on unload.
- **States**: "[REC ]" (white meter, "streaming to output"), "[INIT]" (amber, "connecting input device"), "[PROC]" (sky blue, "transcribing audio stream"), with a blinking text cursor.

### Analog VU (`"vinyl"`)
A warm, vintage VU meter in cream and amber tones with a real dial face.
- **Visualizer**: a spring-loaded needle that kicks toward the audio level and settles with realistic ballistics (the same critically-damped spring used for load/unload animations), sweeping across a `-20` to `+3` scale.
- **Target indicator**: a caption beneath the meter face showing the active target label.
- **Load/unload**: the panel fades in and settles upward slightly into place on load, and reverses on unload.
- **States**: red LED + needle resting at `-20` (idle/standby), amber LED (initializing), blue LED with the needle sweeping on its own (processing), red LED with the needle tracking your voice (recording).

### Speaking Pill

While TTS is responding, a green "SYSTEM RESPONDING" pill slides up from the bottom of the overlay with a live mini-equalizer and the active target label, and slides back down when speech ends.

### Command Trigger Overlay Pill

When a voice command trigger is activated (e.g. saying *"VoxCtrl notes Help me!"*), a purple/indigo glassmorphism command pill slides up from the bottom of the overlay window:
- **Visuals**: Dark purple/indigo background (`rgba(30, 16, 60, 0.94)`) with a glowing purple border (`#a855f7`) and a flashing lightning bolt icon (`⚡`).
- **Content**: Displays the executed command target name (e.g. `NOTES` or `MEETING JOURNAL`) and a summary of the text payload (`▸ Help me!`).
- **Duration**: Auto-dismisses after a configurable duration (default: 3 seconds, configurable via `config.ui.command_overlay_duration_secs`).
- **Toggle**: Controlled via **Settings → Visual Feedback → Show overlay on voice command trigger** (`config.ui.show_command_overlay`).

---

## Custom Overlay Styles

Beyond the eight built-in styles, VoxCtrl loads user-authored styles from
an overlays folder — `dirs::data_local_dir()/voxctrl/overlays` (Rust's
`dirs` crate; e.g. `~/.local/share/voxctrl/overlays` on Linux) — via
`custom_overlays::overlays_dir()` in `src-tauri/src/custom_overlays.rs`.
Every subfolder containing an `index.html` + `style.css` becomes a
selectable style in **Settings → Visual & Feedback → Overlay style**,
named after the folder (`commands::get_custom_overlays`, consumed by
`VisualTab.svelte`'s `overlayStyleOptions` and rendered by
`Overlay.svelte`'s `activeCustomOverlay` branch).

Every launch, `custom_overlays::refresh_bundled_example` (called before
`run()` builds the Tauri app) overwrites `README.md` and the `Custom/`
example — a copy of the built-in Voice Card style with one line changed
(`VOXCTRL` → `CUSTOM OVERLAY`) — from the template files under
`src-tauri/assets/custom-overlay-template/`. This is intentionally
unconditional, not a first-run-only seed: an earlier version only wrote
those files once, which meant a `Custom/` folder from a previous run
never picked up template fixes or improvements shipped in a later build —
exactly the kind of silent staleness this is meant to rule out. Nothing
else in the overlays folder is touched, so any *other* style the user has
created is left alone; `Custom/` itself is documented (in the README this
writes and in `Custom/index.html`'s own comments) as an app-maintained
reference reset on every launch — duplicate it under a new name to keep
your own edits rather than editing it in place.

A custom overlay's `index.html` gets `{{target}}` / `{{trigger}}`
placeholder substitution on its first render (the active routing target's
label). Everything that changes afterward — recording/processing/speaking
state, the live audio level — has to be read from CSS: VoxCtrl continuously
writes it onto custom properties on the page root (`--voxctrl-recording`,
`--voxctrl-processing`, `--voxctrl-speaking`, `--voxctrl-mcp-recording`,
`--voxctrl-audio-ready`, all 0/1, plus `--voxctrl-audio-level` 0..1), for
`var()`/`calc()` to consume directly — see the shipped `Custom/index.html`
and `style.css` for a fully commented, working example (the on/off flip,
the status-stamp text swap, and the audio-reactive LED matrix are all
driven this way, with no script).

This is CSS-only because it has to be: the window's `script-src 'self'`
content-security-policy (`src-tauri/tauri.conf.json`) blocks inline
`<script>` execution everywhere in the app, custom overlays included, with
no visible error in the (console-less) overlay window — a `<script>`-based
overlay just silently never appears. `Overlay.svelte` does still dispatch
`voxctrl-status` / `voxctrl-audio-level` / `voxctrl-cleanup` `CustomEvent`s
on `window` for an overlay's own script to listen for, but nothing in this
app can currently execute that script, so treat those events as unusable
under the shipped CSP rather than as the documented way to build one.

