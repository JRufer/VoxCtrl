# UI & Windows

**Frontend:** `src/` (Svelte 5, Tailwind CSS 4, Vite 5)

## Window Layout

VoxCtr opens three separate native windows managed by Tauri:

| Window | Route | Default Size | Properties |
|---|---|---|---|
| Settings | `/settings` | 840 × 640 | Resizable, standard chrome |
| Overlay | `/overlay` | 560 × 160 | Transparent, always-on-top, no decorations, non-focusable |
| History | `/history` | 600 × 500 | Resizable, standard chrome |

Windows are declared in `src-tauri/tauri.conf.json`.

---

## Settings Window

The main configuration interface. Organized into tabs:

### General Tab
- Overlay style selector (Ocean Wave, Voice Card, Waveform, Pulse)
- Auto-show overlay on recording toggle
- Desktop notification toggle
- History tracking toggle
- Word count display

### Audio Tab
- Input device selector (lists all CPAL devices)
- Gain slider (0.0 – 4.0)
- VAD threshold slider
- Dynamic stream toggle
- Live audio level meter (VU meter, updates from `audio-level` events)
- "Test Microphone" button

### Visual Tab
- Preview of each overlay animation style
- Overlay opacity and size controls

### Routing Tab
- Visual editor for `targets.toml`
- Add/edit/delete output targets
- Per-target fields: label, delivery type, type-specific options
- Per-target processing overrides

### Hotkeys Tab
- Visual editor for `bindings.toml`
- Add/edit/delete hotkey bindings
- Key combo recorder (press keys to set)
- Gesture selector
- Multi-target assignment

### Engine Tab
- Backend selector (Whisper / Moonshine)
- Model size selector with download status
- Compute device selector (auto / CPU / CUDA / Vulkan)
- Language selector
- "Download Model" button with progress indicator

### Ollama Tab
- Enable/disable toggle
- Endpoint URL
- Model name
- Mode selector
- Custom prompt text area
- "Test Connection" button → shows available models

### TTS Tab
- Enable/disable toggle
- Engine selector (Piper / Espeak)
- Voice selector with download status per voice
- "Download Voice" button per voice
- Stop key configuration
- "Test Voice" button

### About Tab
- Version information
- Links to documentation
- License info

---

## Overlay Window

A transparent, always-on-top floating HUD that visualizes audio activity. It has no title bar, cannot be focused by clicking, and auto-shows/hides based on recording state.

### Visualization Styles

#### Ocean Wave
Animated flowing wave in blue/cyan tones. Wave amplitude responds to microphone input level.

#### Voice Card
A minimal card with a pulsing animated circle and the current status text ("Listening...", "Processing...", "Speaking...").

#### Waveform
Classic oscilloscope-style waveform visualization. The amplitude tracks the real-time audio level.

#### Pulse
A series of animated bars (like an equalizer) that pulse in response to audio input.

### Overlay Auto-Behavior
Controlled by `ui.auto_show_overlay`:
- `true` — overlay appears automatically when recording starts and hides when text is delivered
- `false` — overlay must be shown manually via the tray icon or IPC

---

## History Window

Displays a log of all transcription sessions in reverse-chronological order.

Each entry shows:
- Timestamp
- Transcribed text (truncated with expand option)
- Target that received the text
- Word count
- Inference time (ms)
- Detected language

Features:
- Click an entry to copy text to clipboard
- "Clear History" button
- Persists in memory until the app is closed (not written to disk by default)

---

## Frontend State Management

### Config Store (`src/stores/config.ts`)

```typescript
// Reactive store — all Settings components read from this
export const config = writable<AppConfig>(defaultConfig);

// Auto-save: debounced 400ms after any change
config.subscribe(debounce(async (cfg) => {
  await invoke('save_config', { config: cfg });
}, 400));
```

The store is initialized by calling `get_config()` on startup. The `config-changed` Tauri event refreshes it when another window or external process modifies the config.

### Status Store (`src/stores/status.ts`)

```typescript
export const status = writable<AppStatus>({
  recording: false,
  processing: false,
  speaking: false,
  word_count: 0,
  active_target: '',
});

// Updated every 250ms from backend status-tick events
listen('status-tick', (event) => status.set(event.payload));
```

---

## Tauri Events (Backend → Frontend)

| Event | Payload | Description |
|---|---|---|
| `status-tick` | `AppStatus` | Periodic state update (every 250ms) |
| `config-changed` | `AppConfig` | Config was modified externally |
| `audio-level` | `f32` | RMS audio level for VU meter |

---

## Build & Dev

```bash
# Development (hot-reload frontend, Rust recompiles on save)
npm run tauri dev

# Production build
npm run tauri build
```

The Vite dev server runs on `http://localhost:5173`. In dev mode, Tauri loads the frontend from Vite. In production, frontend assets are bundled into the binary.
