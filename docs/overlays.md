# Overlay UI Guide

VoxCtr displays a visual overlay while the microphone is active or while TTS is speaking. The overlay is a dedicated, borderless, transparent Tauri window (560×160 px, transparent background, always-on-top, click-through) that renders a Svelte component over whatever application is in focus.

---

## How the Overlay Works

The overlay is a separate Tauri window running the `/overlay` route. It is configured with the following properties:

- **Transparent** — The window background is fully transparent; only the component's drawn pixels are visible.
- **Always-on-Top** — Floats persistently above all other active desktop windows.
- **Click-Through** (`pointer-events: none`) — Mouse and keyboard events pass cleanly through to the window beneath it, preventing any focus interruption.
- **Non-Resizable, Skip Taskbar** — Does not appear in the desktop panel/taskbar and cannot be manually resized.

The overlay is controlled by the `show_overlay` / `hide_overlay` IPC commands and is automatically shown when recording starts and hidden when transcription completes (provided `ui.show_overlay` is enabled).

The active style is determined by `config.ui.overlay_style`. The window switches styles instantly without restarting—it temporarily unmounts the visualizer and remounts the new template after a 25ms flush to clear the WebKit compositor's frame buffer, preventing graphical ghosts on platforms like Linux (WebKitGTK/Wayland).

---

## Built-in Styles

Four built-in styles are available. The default is **Ocean Wave**.

### Ocean Wave *(default — `"blue_wave"`)*
A layered animated ocean-wave visualization that reacts to real-time audio levels.
- Three overlapping SVG wave layers in deep blue, cyan-aqua, and ice-teal.
- Waves swell in amplitude in response to the `audio-level` event stream from the Rust backend.
- Displays a "Voice Activity" label and a green routing target badge inside a dark rounded card.
- Slides in with a subtle upward ease transition on appear.

### Voice Card (`"voice_card"`)
A symmetric, capsule-shaped spectrum equalizer card.
- 45 vertical visualizer bars that respond to RMS audio level with a fast-attack, fast-release physics envelope.
- Displays the active routing target badge and HSL-gradient bars (purple-pink-rose).
- Built with a sleek dark-obsidian glassmorphism card.

### Waveform (`"waveform"`)
A centre-weighted bar graph with a Gaussian amplitude envelope.
- 48 bars, tallest in the centre and tapering at the edges.
- Adds randomized per-bar noise during active recording for a premium spectrum-analyser look.
- Transitions to a flat idle glide state when not actively recording.

### Pulse Ring (`"pulse"`)
A minimalist target-tracking indicator.
- Three visual states: **Initializing** (amber, "Connecting Mic…"), **Recording** (orange, active target label), **Processing** (blue, "Processing…").
- Dual concentric SVG rings pulse and fade outward during active recording, scaling in glow intensity relative to voice amplitude.

---

## 🎨 User-Creatable Custom Overlay Templates

VoxCtr supports **user-creatable, customizable visual overlays** loaded dynamically from a local directory at runtime. You can build visualizers using plain HTML templates, vanilla CSS styling, and standard JavaScript, with high-performance access to the real-time audio stream.

### 1. File Structure & Path

Custom overlays are scanned dynamically at launch from your local share directory:
* **Linux**: `~/.local/share/voxctl/overlays/`
* **macOS**: `~/Library/Application Support/ai.voxctl.app/overlays/`
* **Windows**: `AppData\Local\ai.voxctl.app\overlays\`

To create your own overlay, create a new subfolder under `overlays/`. The folder name serves as the overlay's ID. Inside the folder, place two files:
```
overlays/
└── my-visualizer/
    ├── index.html   <-- Overlay structure & JS animations
    └── style.css    <-- Overlay styling & layouts
```

> [!NOTE]
> **Bundled Example:** VoxCtr automatically bundles and extracts a premium `gradient-wave` custom overlay folder inside your local directory at launch. Use this folder as a live reference!

---

### 2. HTML Templates & Placeholders

Your `index.html` template can include HTML layouts, inline SVG vectors, and local scripts. VoxCtr automatically scans and replaces the following reactive placeholders at runtime:

- `{{trigger}}` — Replaced with the active global hotkey trigger label (e.g. `Meta + Space`).
- `{{target}}` — Replaced with the active target delivery name (e.g. `Focused Window`).

---

### 3. High-Performance CSS Custom Variables

VoxCtr binds high-speed, reactive CSS custom properties directly to the `.overlay-root` container at 60fps. You can use these variables inside your `style.css` to build pure, GPU-accelerated CSS animations:

| CSS Custom Variable | Value Range | Purpose |
| :--- | :--- | :--- |
| `--voxctr-audio-level` | `0.0` to `1.0` | Real-time interpolated microphone amplitude level. |
| `--voxctr-recording` | `0` or `1` | Indication if the microphone is actively recording. |
| `--voxctr-processing` | `0` or `1` | Indication if the backend is actively performing inference/thinking. |
| `--voxctr-speaking` | `0` or `1` | Indication if the Piper neural TTS voice is speaking. |
| `--voxctr-audio-ready` | `0` or `1` | Indication if the audio system is fully initialized. |

#### CSS Usage Example:
```css
.indicator {
  /* Scale element size dynamically on microphone volume level */
  transform: scale(calc(1.0 + var(--voxctr-audio-level) * 0.5));
  /* Transition color based on recording status */
  background: color-mix(in srgb, #ff6b35 calc(var(--voxctr-recording) * 100%), #38bdf8);
}
```

---

### 4. JavaScript Custom Event Bus

For advanced visualizers using HTML5 `<canvas>`, WebGL, or SVG path morphing, standard vanilla custom DOM events are dispatched on the `window` object:

#### `voxctr-audio-level`
Dispatched in real-time when fresh amplitude inputs are received.
* **Payload (`event.detail`)**: `number` (raw RMS audio energy, roughly `0.0` to `1.0`).

```javascript
window.addEventListener("voxctr-audio-level", (e) => {
  const rawVolume = e.detail;
  // Drive custom visual animations
});
```

#### `voxctr-status`
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
window.addEventListener("voxctr-status", (e) => {
  const { recording, audio_level, active_target_label } = e.detail;
  if (recording) {
    // update canvas loop
  }
});
```

---

### 5. Dynamic Script Execution

Svelte's standard template injector blocks `<script>` blocks for security reasons. To make user-creatable visualizers fully programmable, VoxCtr runs a custom action on mount:
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
mkdir -p ~/.local/share/voxctl/overlays/glow-ring
```

### Step 2: Write `index.html`
Save this file as `~/.local/share/voxctl/overlays/glow-ring/index.html`:
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

    window.addEventListener("voxctr-audio-level", (e) => {
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
Save this file as `~/.local/share/voxctl/overlays/glow-ring/style.css`:
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
1. Open VoxCtr **Settings** -> **Visual & Feedback** -> **Overlay style** dropdown.
2. Select your newly registered `"glow-ring"` overlay!
3. Trigger dictation and watch your custom glow visualizer react smoothly to your voice.
