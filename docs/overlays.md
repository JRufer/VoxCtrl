# Overlay UI Guide

VoxCtr displays a visual overlay while the microphone is active or while TTS is speaking. The overlay is a dedicated Tauri window (560×160 px, transparent background, always-on-top, click-through) that renders a Svelte component over whatever application is in focus.

---

## How the Overlay Works

The overlay is a separate Tauri window running the `/overlay` route. It is:

- **Transparent** — the window background is fully transparent; only the component's drawn pixels are visible.
- **Always-on-top** — floats above all other windows.
- **Click-through** (`pointer-events: none`) — keyboard and mouse events pass through to the window beneath.
- **Non-resizable, skip-taskbar** — does not appear in the taskbar and cannot be resized.

The overlay is controlled by the `show_overlay` / `hide_overlay` IPC commands and is automatically shown when recording starts and hidden when transcription completes (when `ui.show_overlay` is enabled in config).

The active component is determined by `config.ui.overlay_style`. The overlay switches between styles without restarting the window — it remounts the component after a 25 ms flush to clear the WebKit compositor buffer.

---

## Built-in Styles

Four styles are available. The default is **Blue Wave**.

### Blue Wave *(default — `"blue_wave"`)*

A layered animated ocean-wave visualization that reacts to real-time audio levels.

- Three overlapping SVG wave layers in deep blue, cyan-aqua, and ice-teal
- Waves rise and swell in amplitude in response to the `audio-level` event stream from the Rust backend
- Displays a "Voice Activity" label and a green routing target badge in a dark rounded card
- Slides in with a subtle upward animation on appear

### Voice Card (`"voice_card"`)

A bar-graph spectrum display with 45 animated bars.

- Bars respond to RMS audio level with a fast-attack, fast-release envelope
- Displays "Voice Activity" label and active routing target badge
- Dark glassy card with rounded corners

### Waveform (`"waveform"`)

A centre-weighted bar graph with a Gaussian amplitude envelope.

- 48 bars, tallest in the centre and tapering at the edges
- Randomised per-bar noise during recording for a spectrum-analyser look
- Shows a flat idle state when not recording

### Pulse (`"pulse"`)

A compact pill-shaped indicator with animated concentric rings.

- Three states: **Initializing** (amber, shows "Connecting Mic…"), **Recording** (orange, shows active target label), **Processing** (blue, shows "Processing…")
- Two animated CSS rings pulse outward during active recording
- Includes an active target badge with context-appropriate icon (🎯 recording, 🧠 processing, ⏳ initializing)

### None (`"none"`)

Disables the overlay entirely. The window remains hidden during recording.

---

## Selecting an Overlay Style

1. Open **⚙ Settings** from the system tray icon.
2. Go to the **Visual** tab.
3. In the **Overlay Appearance** section, choose a style from the dropdown.
4. The setting saves automatically and takes effect immediately — no restart required.

---

## Configuration

The overlay style is stored in `~/.config/voxctl/config.json` under `ui.overlay_style`:

```json
{
  "ui": {
    "show_overlay": true,
    "overlay_style": "blue_wave"
  }
}
```

Valid values for `overlay_style`: `"blue_wave"` (default), `"voice_card"`, `"waveform"`, `"pulse"`, `"none"`.

Setting `show_overlay` to `false` prevents the overlay window from being shown at all, regardless of the style setting.

---

## Audio Level Feed

All overlay styles that react to audio receive levels via the `audio-level` Tauri event, emitted by the Rust backend approximately every 50 ms. The payload is a `number` (RMS energy, roughly 0.0–1.0). Components multiply this by a calibration factor (typically `100.0`) to map it to a usable animation amplitude.

```typescript
import { listen } from '@tauri-apps/api/event';

await listen<number>('audio-level', (event) => {
  const level = Math.min(1.0, event.payload * 100.0);
  // drive animation
});
```

The `audio-level` stream is only active when monitoring is enabled. The overlay starts monitoring automatically when the recording session begins.

---

## IPC Commands

```typescript
import { invoke } from '@tauri-apps/api/core';

// Show the overlay window and set always-on-top
await invoke('show_overlay');

// Hide the overlay window
await invoke('hide_overlay');
```

Both commands are also called automatically by the recording pipeline when `ui.show_overlay` is `true`.
