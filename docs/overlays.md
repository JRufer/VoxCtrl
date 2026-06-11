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

---

## 🎨 User-Creatable Custom Overlay Templates

> [!NOTE]
> Custom overlays are HTML/CSS/JS templates rendered by the web (Svelte) overlay layer, whereas the four built-in styles above are rendered by the native `voxctrl-overlay` helper. Custom overlay rendering is being migrated to the new native overlay engine; the template format documented below stays the same.

VoxCtrl supports **user-creatable, customizable visual overlays** loaded dynamically from a local directory at runtime. You can build visualizers using plain HTML templates, vanilla CSS styling, and standard JavaScript, with high-performance access to the real-time audio stream.

### 1. File Structure & Path

Custom overlays are scanned dynamically at launch from your local share directory:
* **Linux**: `~/.local/share/voxctrl/overlays/`
* **macOS**: `~/Library/Application Support/ai.voxctrl.app/overlays/`
* **Windows**: `AppData\Local\ai.voxctrl.app\overlays\`

To create your own overlay, create a new subfolder under `overlays/`. The folder name serves as the overlay's ID. Inside the folder, place two files:
```
overlays/
└── my-visualizer/
    ├── index.html   <-- Overlay structure & JS animations
    └── style.css    <-- Overlay styling & layouts
```

> [!NOTE]
> **Bundled Example:** VoxCtrl automatically bundles and extracts a premium `gradient-wave` custom overlay folder inside your local directory at launch. Use this folder as a live reference!

---

### 2. HTML Templates & Placeholders

Your `index.html` template can include HTML layouts, inline SVG vectors, and local scripts. VoxCtrl automatically scans and replaces the following reactive placeholders at runtime:

- `{{trigger}}` — Replaced with the active global hotkey trigger label (e.g. `Meta + Space`).
- `{{target}}` — Replaced with the active target delivery name (e.g. `Focused Window`).

---

### 3. High-Performance CSS Custom Variables

VoxCtrl binds high-speed, reactive CSS custom properties directly to the `.overlay-root` container at 60fps. You can use these variables inside your `style.css` to build pure, GPU-accelerated CSS animations:

| CSS Custom Variable | Value Range | Purpose |
| :--- | :--- | :--- |
| `--voxctrl-audio-level` | `0.0` to `1.0` | Real-time interpolated microphone amplitude level. |
| `--voxctrl-recording` | `0` or `1` | Indication if the microphone is actively recording. |
| `--voxctrl-processing` | `0` or `1` | Indication if the backend is actively performing inference/thinking. |
| `--voxctrl-speaking` | `0` or `1` | Indication if the Piper neural TTS voice is speaking. |
| `--voxctrl-mcp-recording` | `0` or `1` | Indication if the MCP server is actively recording/listening to the microphone. |
| `--voxctrl-audio-ready` | `0` or `1` | Indication if the audio system is fully initialized. |

#### CSS Usage Example:
```css
.indicator {
  /* Scale element size dynamically on microphone volume level */
  transform: scale(calc(1.0 + var(--voxctrl-audio-level) * 0.5));
  /* Transition color based on recording status */
  background: color-mix(in srgb, #ff6b35 calc(var(--voxctrl-recording) * 100%), #38bdf8);
}
```

---

### 4. JavaScript Custom Event Bus

For advanced visualizers using HTML5 `<canvas>`, WebGL, or SVG path morphing, standard vanilla custom DOM events are dispatched on the `window` object:

#### `voxctrl-audio-level`
Dispatched in real-time when fresh amplitude inputs are received.
* **Payload (`event.detail`)**: `number` (raw RMS audio energy, roughly `0.0` to `1.0`).

```javascript
window.addEventListener("voxctrl-audio-level", (e) => {
  const rawVolume = e.detail;
  // Drive custom visual animations
});
```

#### `voxctrl-status`
Dispatched at 60fps containing a full state object.
* **Payload (`event.detail`)**:
  ```json
  {
    "recording": true,
    "processing": false,
    "speaking": false,
    "audio_ready": true,
    "active_target_label": "Focused Window",
    "audio_level": 0.45
  }
  ```

```javascript
window.addEventListener("voxctrl-status", (e) => {
  const { recording, audio_level, active_target_label } = e.detail;
  if (recording) {
    // update canvas loop
  }
});
```

---

### 5. Dynamic Script Execution

Svelte's standard template injector blocks `<script>` blocks for security reasons. To make user-creatable visualizers fully programmable, VoxCtrl runs a custom action on mount:
* It extracts all script blocks inside your `index.html`.
* Re-compiles them dynamically.
* Re-injects them into the DOM frame to force secure execution.

This allows you to write high-performance loops (such as `requestAnimationFrame` render cycles) inside standard `<script>` tags natively, without requiring webpack, node_modules, or external bundlers.

---

### 6. Naming Conflict Resolution

To prevent custom layouts from overriding or breaking the core built-in application visualizers, the scanning system implements automatic **conflict resolution**:
* Folder names matching built-in styles (`waveform`, `pulse`, `blue_wave`, `voice_card`, `none`) are reserved.
* If a user names their folder exactly `pulse`, the scanner automatically registers it as `pulse_custom` in the styles dropdown. This ensures both visualizers can coexist and run flawlessly.

---

## 🚀 Step-by-Step Custom Visualizer Tutorial

Below is a complete, copy-pasteable example of a custom visualizer card featuring a glowing circular audio-reactive indicator.

### Step 1: Create the Folder
Create a folder inside your local directory named `glow-ring`:
```bash
mkdir -p ~/.local/share/voxctrl/overlays/glow-ring
```

### Step 2: Write `index.html`
Save this file as `~/.local/share/voxctrl/overlays/glow-ring/index.html`:
```html
<div class="custom-card">
  <div class="meta-row">
    <span class="title">My Glow HUD</span>
    <span class="target-badge">{{target}}</span>
  </div>

  <div class="visualizer-container">
    <div class="glow-circle" id="myGlowCircle"></div>
  </div>

  <script>
    let currentVolume = 0;
    let targetVolume = 0;

    window.addEventListener("voxctrl-audio-level", (e) => {
      // Scale level up for visual display
      targetVolume = Math.min(1.0, e.detail * 100.0);
    });

    function draw() {
      // Smooth linear interpolation for visuals
      currentVolume += (targetVolume - currentVolume) * 0.3;
      targetVolume *= 0.85;

      const circle = document.getElementById("myGlowCircle");
      if (circle) {
        // Expand circle and increase glow drop-shadow dynamically
        const scale = 1.0 + currentVolume * 1.5;
        const shadow = currentVolume * 30;
        circle.style.transform = `scale(${scale})`;
        circle.style.boxShadow = `0 0 ${20 + shadow}px rgba(56, 189, 248, ${0.4 + currentVolume * 0.6})`;
      }

      requestAnimationFrame(draw);
    }
    draw();
  </script>
</div>
```

### Step 3: Write `style.css`
Save this file as `~/.local/share/voxctrl/overlays/glow-ring/style.css`:
```css
.custom-card {
  width: 300px;
  height: 110px;
  background: rgba(13, 13, 16, 0.93);
  border: 1.2px solid rgba(255, 255, 255, 0.05);
  border-radius: 20px;
  padding: 12px;
  box-sizing: border-box;
  display: flex;
  flex-direction: column;
  justify-content: space-between;
  box-shadow: 0 12px 32px rgba(0, 0, 0, 0.7);
  backdrop-filter: blur(12px);
}

.meta-row {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.title {
  font-family: sans-serif;
  font-size: 11px;
  color: rgba(255, 255, 255, 0.3);
  font-weight: 700;
  text-transform: uppercase;
}

.target-badge {
  font-family: sans-serif;
  font-size: 10px;
  color: #38bdf8;
  background: rgba(56, 189, 248, 0.1);
  border: 1px solid rgba(56, 189, 248, 0.3);
  border-radius: 4px;
  padding: 2px 6px;
  font-weight: 600;
}

.visualizer-container {
  display: flex;
  align-items: center;
  justify-content: center;
  height: 60px;
  width: 100%;
}

.glow-circle {
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #38bdf8;
  transition: transform 0.05s ease-out;
}
```

### Step 4: Load the Overlay
1. Open VoxCtrl **Settings** -> **Visual & Feedback** -> **Overlay style** dropdown.
2. Select your newly registered `"glow-ring"` overlay!
3. Trigger dictation and watch your custom glow visualizer react smoothly to your voice.
