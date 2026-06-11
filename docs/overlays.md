# Overlay UI Guide

VoxCtrl displays a visual overlay while the microphone is active, while TTS is speaking, or while the MCP server is actively recording (provided Visual Feedback is enabled). The built-in overlay styles are rendered by a dedicated **native helper process** (`voxctrl-overlay`, built with [Slint](https://slint.dev)) in a borderless, transparent, always-on-top, click-through window (560×190 logical px) drawn over whatever application is in focus.

---

## How the Overlay Works

The main VoxCtrl process spawns the `voxctrl-overlay` helper binary at startup and streams newline-delimited JSON messages (`status`, `position`, `shutdown`) to its stdin — recording/processing/speaking state, the smoothed microphone level, the active routing target label, the configured style, and screen coordinates. The helper window has the following properties:

- **Transparent** — The window background is fully transparent; only the drawn pixels of the visualizer are visible.
- **Always-on-Top** — Floats persistently above all other active desktop windows. The window level is re-asserted every time the overlay re-appears, since some window managers reset it across hide/show cycles.
- **Click-Through** — The window's cursor hit-test is disabled at the windowing-system level (winit `set_cursor_hittest(false)`), so mouse events pass cleanly through to the window beneath it, preventing any focus interruption.
- **Borderless / Frameless** — No title bar or decorations.

The overlay is shown automatically when recording starts and hidden when transcription completes (provided `ui.show_overlay` is enabled). The active style is determined by `config.ui.overlay_style` and is hot-switched without restarting the helper.

### Load & Unload Animations

Every built-in style plays a dedicated load animation when it appears and an unload animation when it disappears. Animations are driven by a slightly-underdamped spring (so overlays land with a subtle bounce), and the helper window intentionally **stays alive until the unload animation finishes** instead of vanishing on the same frame recording stops. Each style interprets the spring's progress in its own way — see the per-style descriptions below.

---

## Screen Positioning

The visual presentation overlay window can be positioned dynamically on the active monitor where your mouse cursor or focused application is located. 

Users can configure the screen alignment under **Settings** -> **Visual & Feedback** -> **Overlay position** or manually in `config.json` via `ui.overlay_position`.

Available screen positions:
- **`"center"` (Default)** — Positions the overlay at the exact horizontal and vertical center of the active monitor.
- **`"top"`** — Positions the overlay at the horizontal center, aligned `60` logical pixels from the top of the monitor.
- **`"bottom"`** — Positions the overlay at the horizontal center, aligned `60` logical pixels from the bottom of the monitor.

Tauri dynamically calculates physical pixel values taking into account your display's current high-DPI scaling factor (`scale_factor`), ensuring perfect resolution-independent positioning on 1080p, 1440p, or 4K monitors. Changes to the position setting are instantly applied in real-time if the overlay window is currently visible.

---

## Target Display / Multi-Monitor Support

In multi-monitor setups, VoxCtrl allows you to specify exactly which display screen the visual overlay should appear on.

You can configure the target display under **Settings** -> **Visual & Feedback** -> **Overlay display** or manually in `config.json` via `ui.overlay_monitor`.

Options:
- **`"primary"` (Default)** — Constrains the overlay to the OS-defined primary display screen.
- **Specific Monitor Name (e.g. `"HDMI-1"`, `"DP-2"`)** — Connected displays are dynamically queried once at application startup. Selecting one of these binds the visualizer to that specific panel.

### Graceful Disconnection Failover

If the configured target monitor is unplugged or disconnected at runtime, the Tauri backend will automatically fail over to the **Primary Monitor** to keep the visualizer fully accessible. When this happens, a golden warning badge will be displayed inside the settings UI to alert you that the target display is disconnected and fallback mode is active.

---

## Built-in Styles

Four built-in styles are available — each with its own visual identity, its own kind of audio visualizer, its own load/unload animation, and a clear indicator of the active routing target. The default is **Ocean Wave**.

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

### Speaking Pill

While TTS is responding, a green "SYSTEM RESPONDING" pill slides up from the bottom of the overlay with a live mini-equalizer and the active target label, and slides back down when speech ends.

