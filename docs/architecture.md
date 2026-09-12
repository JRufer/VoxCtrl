# Architecture

## High-Level Design

VoxCtrl is a **Tauri 2** application: a compiled Rust backend that spawns a WebView window running a Svelte SPA. The two halves communicate via Tauri's IPC bridge (invoke commands + event emitters).

```
┌─────────────────────────────────────────────────────────┐
│                    Tauri Desktop App                     │
│                                                          │
│  ┌───────────────────┐      ┌──────────────────────────┐ │
│  │   Svelte Frontend │◄────►│    Rust Backend (lib.rs) │ │
│  │   (WebView)       │ IPC  │    + crates workspace     │ │
│  └───────────────────┘      └──────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
         │                              │
   Settings                      Audio devices,
   windows                       Filesystem, DBus,
                                 Network (OpenAI API/HTTP)
```

In addition, the backend spawns a **native overlay helper process** (`voxctrl-overlay`, built with Slint from `src-tauri/src/overlay.rs`) that renders the always-on-top, click-through recording HUD. The backend streams newline-delimited JSON (`status` / `position` / `shutdown`) to the helper's stdin; the helper runs its own 16 ms animation loop, drives spring-based load/unload animations, and builds the per-style visualizer geometry (oscilloscope trace, radar sweep, ocean waves, VU LED matrix) each tick.

---

## Crate Workspace

The backend is organized as a Cargo workspace of focused, single-responsibility crates:

```
VoxCtrl/
├── src-tauri/         # Tauri app entry + IPC command handlers
│   └── src/
│       ├── main.rs    # App bootstrap
│       ├── lib.rs     # Pipeline coordinator
│       ├── commands.rs# Tauri #[command] handlers
│       └── state.rs   # Shared AppState
│
└── crates/
    ├── voxctrl-config/     # AppConfig struct, TOML/JSON persistence
    ├── voxctrl-audio/      # Microphone capture, resampling, VU meter
    ├── voxctrl-hotkeys/    # Global shortcuts (XDG portal / evdev / Windows hook / D-Bus)
    ├── voxctrl-inference/  # whisper.cpp/Moonshine/Parakeet/Remote STT + post-processing
    ├── voxctrl-routing/    # OutputTarget + HotkeyBinding data models, 11-target router
    ├── voxctrl-inject/     # Text injection orchestrator (Wayland/X11/Windows)
    ├── voxctrl-winput/     # Windows native synthesised Unicode keyboard input
    ├── voxctrl-tts/        # Neural & local TTS (Breeze/Piper/Pocket/Inflect/VoxCPM2/eSpeak)
    ├── voxctrl-mcp/        # MCP JSON-RPC server (Unix socket / named pipe)
    ├── voxctrl-dbus/       # DBus service (Linux session bus)
    ├── voxctrl-llm/        # OpenAI-compatible LLM HTTP client
    ├── voxctrl-text/       # Shared snippet expansion + fuzzy vocab correction
    ├── voxctrl-update/     # GitHub release check, download, verify, self-replace
    ├── voxctrl-bugreport/  # Diagnostic collection, allowlist redaction, telemetry-free reporting
    └── voxctrl-llm-sidecar/# Vulkan-accelerated llama.cpp sidecar for S1-mini dictation cleanup
```

---

## Data Flow

```
Hotkey press
     │
     ▼
voxctrl-hotkeys ──gesture_tx──► lib.rs coordinator
                                      │
                         ┌────────────┤
                         │            │
                  Start AudioRecorder  Determine target from binding
                  (voxctrl-audio)
                         │
                    audio_tx chunks
                         │
                         ▼
                  Audio Accumulator
                  (lib.rs buffer)
                         │
                  Hotkey release / VAD stop
                         │
                    inference_tx
                         │
                         ▼
                  InferenceEngine.process()
                  (voxctrl-inference)
                    │  Noise gate (VAD)
                    │  Speech transcription (whisper.cpp / Moonshine / Parakeet / Remote)
                    │  Filler removal
                    │  Spoken punctuation
                    │  Auto-format lists
                    │  Snippet expansion
                    │  Custom vocab fuzzy correction
                    │  Code mode
                    │  Silence hallucination filter
                    │  LLM rewrite via OpenAI API (optional, per-target)
                    │  S1-mini text normalization via voxctrl-llm-sidecar
                         │
                    text_tx (InferenceOutput)
                         │
                         ▼
                  OutputTargetRouter.route()
                  (voxctrl-routing)
                    ├── inject → voxctrl-inject / voxctrl-winput
                    ├── clipboard → arboard
                    ├── file → tokio::fs
                    ├── http/webhook → reqwest
                    ├── exec → std::process
                    ├── socket → UnixStream / TcpStream
                    ├── dbus → voxctrl-dbus
                    ├── mcp → voxctrl-mcp response queue
                    ├── pipe → named FIFO
                    ├── speak → voxctrl-tts
                    └── chat → voxctrl-llm conversational loop
                         │
                    Tauri event → frontend
                    (status-tick)
```

---

## Concurrency Model

VoxCtrl uses Tokio for async I/O plus dedicated OS threads for latency-sensitive work:

| Thread / Task | Type | Purpose |
|---|---|---|
| Main Tauri thread | OS thread | Window management, IPC dispatch |
| Audio capture | OS thread (cpal) | Microphone streaming at hardware rate |
| Audio level emitter | OS thread | Forwards RMS levels to the UI and the overlay, coalesced to one per frame (16 ms); silent unless recording or monitoring |
| Hotkey listener | async task + OS threads | XDG GlobalShortcuts portal, with an evdev/Win32 fallback |
| Inference worker | OS thread | Blocking Whisper computation; `WhisperState` (KV cache + attention buffers) is allocated once at load and reused across all calls |
| Status ticker | Tokio task | Ticks at 150 ms to animate the tray icon; emits `status-tick` only when the state changed, plus a 900 ms heartbeat |
| Config watcher | Tokio task | `inotify`/`kqueue` on config files |
| MCP server | Tokio task | Unix socket accept loop |
| DBus service | Tokio task | Session bus method handler |
| TTS FIFO watcher | Tokio task | Named pipe reader for TTS input |

**Shared state** is an `Arc<AppState>` with `AtomicBool`/`AtomicU32` for hot-path flags and `Mutex` for heavier data (targets, TTS handle).

**Channels** (crossbeam/tokio):
- `audio_tx` / `audio_rx` — `Vec<f32>` chunks
- `inference_tx` / `inference_rx` — `InferenceRequest`
- `text_tx` / `text_rx` — `InferenceOutput`
- `audio_level_tx` / `level_rx` — `f32` RMS
- `gesture_tx` / `gesture_rx` — `GestureEvent`
- `hotkey_reloader` — updated bindings list sent to listener thread (hot-reload)

---

## Frontend Architecture

The Svelte frontend is a single-page app with three logical "pages" rendered as separate Tauri windows:

```
App.svelte  (route switcher)
  ├── /settings  → Settings component (sidebar with 9 tabs)
  │     ├── GeneralTab
  │     ├── EngineTab
  │     ├── RoutingTab     (targets + bindings editor)
  │     ├── VisualTab
  │     ├── AudioTab
  │     ├── TtsTab
  │     ├── FeaturesTab
  │     ├── OpenAiTab  (labeled "OpenAI API")
  │     └── AboutTab
  │
  ├── /wizard    → SetupWizard component (first-run setup, 7 steps)
  │     ├── WelcomeStep
  │     ├── EngineStep     (engine + model, downloads before continuing)
  │     ├── HotkeyStep     (gesture + key capture, desktop registration)
  │     ├── OverlayStep    (style + position, bundled webm previews)
  │     ├── TestStep       (live end-to-end dictation)
  │     ├── VoiceStep      (optional TTS engine, per-card download)
  │     └── DoneStep       (summary + anything that failed)
  │
  ├── /overlay   → Overlay component (web overlay layer; the on-screen HUD
  │     │          for built-in styles is the native voxctrl-overlay helper)
  │     ├── BlueWave       (default — "Ocean Wave" tide pool)
  │     ├── VoiceCard      ("Voice Card" VU LED matrix card)
  │     ├── Waveform       (green-phosphor oscilloscope)
  │     └── Pulse          ("Pulse Ring" sonar dial)
  │
```

**State management:**
- `src/stores/config.ts` — reactive `AppConfig` with 400ms debounced auto-save via `save_config` IPC; also listens for `config-changed` events
- `src/stores/status.ts` — live state from `status-tick` events + derived stores (`recording`, `speaking`, `wordCount`, `activeTargetLabel`). Falls back to polling `get_status` once a second when ticks have been absent for two, so a window the events do not reach still shows live state; while ticks are arriving it does no IPC at all

---

## File Locations

| Path | Contents |
|---|---|
| `~/.config/voxctrl/config.json` | Main application config |
| `~/.config/voxctrl/targets.toml` | Output target definitions |
| `~/.config/voxctrl/bindings.toml` | Hotkey binding definitions |
| `~/.local/share/voxctrl/models/` | Downloaded Whisper GGUF models |
| `~/.local/share/voxctrl/piper-voices/` | Downloaded Piper voice packs |
| `~/.local/share/voxctrl/models/inflect-micro/` | Inflect-Micro-v2 ONNX graphs and symbol list |
| `/tmp/voxctrl-mcp.sock` | MCP Unix domain socket (Linux) |
| `\\.\pipe\voxctrl-mcp` | MCP named pipe (Windows) |
