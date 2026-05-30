# UI & Windows

**Frontend:** `src/` (Svelte 5, Tailwind CSS 4, Vite 5)

## Window Layout

VoxCtr opens three separate native windows managed by Tauri:

| Window | Route | Default Size | Properties |
|---|---|---|---|
| Settings | `/settings` | 840 × 640 (min 700 × 500) | Resizable, standard chrome |
| Overlay | `/overlay` | 560 × 160 | Transparent, always-on-top, no decorations, not resizable, skip taskbar |
| History | `/history` | 600 × 500 | Resizable, standard chrome |

Windows are declared in `src-tauri/tauri.conf.json`. All three start hidden (`visible: false`) and are shown programmatically.

---

## Settings Window

The main configuration interface. Organized into a sidebar with nine tabs:

### General Tab
- Overlay show/hide toggle
- Overlay style selector
- Auto-show settings on startup toggle
- Desktop notification toggle
- History tracking toggle
- Recording status indicator and word count
- Manual record/stop button

### Engine Tab
- Backend selector (`auto`, `whisper-cpp`, `moonshine`)
- Inference mode selector (`Balanced`, `Aggressive`)
- Whisper model size selector with download status
- Compute device selector (auto / CPU / CUDA / Vulkan)
- Thread count control
- Moonshine model/language settings
- "Download Model" button with progress
- **Missing Model Warning & Auto-Redirection**: Startup check programmatically determines if the configured Whisper voice model file is downloaded on the local machine. If missing, it immediately switches the active Settings tab to "Engine" and presents a Tailwind-styled yellow warning alert prompting the user to select and download a GGUF voice model size.

### Routing Tab
- Visual editor for `targets.toml` — add/edit/delete output targets
- Per-target fields: label, delivery type, type-specific options
- Per-target processing override controls
- Hotkey binding management (add/edit/delete bindings, key combo recorder, gesture selector)

### Visual Tab
- Preview and selection of overlay animation styles
- **Overlay Position Control**: Dropdown choice for setting the Heads-Up display screen alignment (**Top**, **Center**, or **Bottom** of the screen).
- Overlay appearance controls

### Audio Tab
- Input device selector (lists all CPAL devices)
- Gain slider
- VAD threshold slider
- Noise suppression toggle
- Dynamic stream toggle
- Live audio level meter (VU meter, updates from `audio-level` events during monitoring)
- Evdev device path input

### TTS Tab
- Enable/disable toggle
- Engine selector (Piper / Espeak)
- Voice selector with download status per voice
- "Download Voice" button per voice
- Stop key configuration
- Response overlay toggle

### Features Tab
- Filler removal toggle
- Spoken punctuation toggle
- Auto-format lists toggle
- Quiet mode toggle
- Custom vocabulary list editor
- Snippet key-value editor

### Ollama Tab
- Enable/disable toggle
- Endpoint URL
- Model name
- Mode selector
- Custom prompt text area
- "Test Connection" button → shows available models

### About Tab
- Version information
- Links to documentation and source

---

## Overlay Window

A transparent, always-on-top floating HUD that visualizes audio activity. It has no title bar, no taskbar entry, is not resizable, and auto-shows/hides based on recording state (controlled by `ui.show_overlay`).

The window coordinates are calculated dynamically relative to the active display monitor's size and scale factor, placing the visualizer cleanly in the **Center**, **Top** (60 logical pixels from the top), or **Bottom** (60 logical pixels from the bottom) of the screen depending on the `ui.overlay_position` setting. Position changes are hot-reloaded and applied instantly in real-time.

### Visualization Styles

Set via `ui.overlay_style` in config:

#### `blue_wave` (default)
Animated flowing wave in blue/cyan tones. Wave amplitude responds to microphone input level. Component: `BlueWave.svelte`.

#### `voice_card`
A minimal card with a pulsing animated circle and the current status text ("Listening...", "Processing...", "Speaking..."). Component: `VoiceCard.svelte`.

#### `waveform`
Classic oscilloscope-style waveform visualization. Amplitude tracks the real-time audio level. Component: `Waveform.svelte`.

#### `pulse`
A series of animated bars (like an equalizer) that pulse in response to audio input. Component: `Pulse.svelte`.

#### `none`
Overlay is disabled entirely.

---

## History Window

Displays a log of all transcription sessions in reverse-chronological order.

Each entry shows:
- Timestamp
- Transcribed text
- Target that received the text
- Inference time (ms)

Features:
- "Clear History" button (also resets word count)
- History persists in memory until the app is closed (not written to disk)

Enabled via `ui.history_enabled = true` in config.

---

## Frontend State Management

### Config Store (`src/stores/config.ts`)

```typescript
// Reactive store — all Settings components bind to this
export const config = writable<AppConfig>(defaultConfig);
export const configDirty = writable(false);

// Auto-save: debounced 400ms after any change
config.subscribe((cfg) => {
  // 400ms debounce → invoke('save_config', { newConfig: cfg })
});
```

Initialized by `loadConfig()` which calls `get_config()` IPC on startup. The `config-changed` Tauri event refreshes the store when another window or external process modifies config, with a guard to avoid circular auto-save loops.

### Status Store (`src/stores/status.ts`)

```typescript
export const status = writable<AppStatus>({
  recording: false,
  processing: false,
  speaking: false,
  audio_ready: true,
  word_count: 0,
  active_target_id: "default",
  active_target_label: "Focused Window",
});

// Derived convenience stores:
export const recording = derived(status, ($s) => $s.recording);
export const speaking = derived(status, ($s) => $s.speaking);
export const wordCount = derived(status, ($s) => $s.word_count);
export const activeTargetLabel = derived(status, ($s) => $s.active_target_label ?? "Focused Window");
```

Updated by `status-tick` Tauri events (emitted by backend every ~250ms) and an initial `get_status()` call on load.

---

## Tauri Events (Backend → Frontend)

| Event | Payload | Description |
|---|---|---|
| `status-tick` | `AppStatus` | Periodic state update (~250ms) |
| `config-changed` | `AppConfig` | Config was modified (by any window or externally) |
| `audio-level` | `f32` | RMS audio level for VU meter (while monitoring is active) |

---

## Build & Dev

```bash
# Development (hot-reload frontend, Rust recompiles on save)
npm run tauri dev

# Production build (AppImage on Linux, .exe/.msi on Windows)
npm run tauri build
```

The Vite dev server runs on `http://localhost:5173`. In dev mode, Tauri loads the frontend from Vite. In production, frontend assets are bundled into the binary.
