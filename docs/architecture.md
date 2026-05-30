# Architecture

## High-Level Design

VoxCtr is a **Tauri 2** application: a compiled Rust backend that spawns a WebView window running a Svelte SPA. The two halves communicate via Tauri's IPC bridge (invoke commands + event emitters).

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
   Settings/Overlay/             Audio devices,
   History windows               Filesystem, DBus,
                                 Network (Ollama/HTTP)
```

---

## Crate Workspace

The backend is organized as a Cargo workspace of focused, single-responsibility crates:

```
VoxCtr/
├── src-tauri/         # Tauri app entry + IPC command handlers
│   └── src/
│       ├── main.rs    # App bootstrap
│       ├── lib.rs     # Pipeline coordinator (~2000 LOC)
│       ├── commands.rs# Tauri #[command] handlers (~330 LOC)
│       └── state.rs   # Shared AppState
│
└── crates/
    ├── voxctr-config/     # AppConfig struct, TOML/JSON persistence
    ├── voxctr-audio/      # Microphone capture, resampling, VU meter
    ├── voxctr-hotkeys/    # Global key listener (evdev / Win32)
    ├── voxctr-inference/  # Whisper transcription + post-processing
    ├── voxctr-routing/    # OutputTarget + HotkeyBinding data models, router
    ├── voxctr-inject/     # Text injection via wtype/xdotool/clipboard
    ├── voxctr-tts/        # Piper/Espeak TTS engine
    ├── voxctr-mcp/        # MCP JSON-RPC server (Unix socket / named pipe)
    ├── voxctr-dbus/       # DBus service (Linux session bus)
    └── voxctr-llm/        # Ollama HTTP client
```

---

## Data Flow

```
Hotkey press
     │
     ▼
voxctr-hotkeys ──gesture_tx──► lib.rs coordinator
                                      │
                         ┌────────────┤
                         │            │
                  Start AudioRecorder  Determine target from binding
                  (voxctr-audio)
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
                  (voxctr-inference)
                    │  Noise gate (VAD)
                    │  Whisper transcription
                    │  Filler removal
                    │  Spoken punctuation
                    │  Auto-format lists
                    │  Snippet expansion
                    │  Custom vocab fuzzy correction
                    │  Code mode
                    │  Silence hallucination filter
                    │  Ollama rewrite (optional, per-target)
                         │
                    text_tx (InferenceOutput)
                         │
                         ▼
                  OutputTargetRouter.route()
                  (voxctr-routing)
                    ├── inject → voxctr-inject
                    ├── clipboard → arboard
                    ├── file → tokio::fs
                    ├── http/webhook → reqwest
                    ├── exec → std::process
                    ├── socket → UnixStream
                    ├── dbus → voxctr-dbus
                    ├── mcp → voxctr-mcp response queue
                    └── pipe → named FIFO
                         │
                    Tauri event → frontend
                    (status-tick, history update)
```

---

## Concurrency Model

VoxCtr uses Tokio for async I/O plus dedicated OS threads for latency-sensitive work:

| Thread / Task | Type | Purpose |
|---|---|---|
| Main Tauri thread | OS thread | Window management, IPC dispatch |
| Audio capture | OS thread (cpal) | Microphone streaming at hardware rate |
| Audio level emitter | Tokio task | Forwards RMS levels to UI every ~50ms |
| Hotkey listener | OS thread | evdev/Win32 event loop |
| Inference worker | OS thread | Blocking Whisper computation; `WhisperState` (KV cache + attention buffers) is allocated once at load and reused across all calls |
| Status ticker | Tokio task | Emits `status-tick` events every 250ms |
| Config watcher | Tokio task | `inotify`/`kqueue` on config files |
| MCP server | Tokio task | Unix socket accept loop |
| DBus service | Tokio task | Session bus method handler |
| TTS FIFO watcher | Tokio task | Named pipe reader for TTS input |

**Shared state** is an `Arc<AppState>` with `AtomicBool`/`AtomicU32` for hot-path flags and `Mutex` for heavier data (targets, history, TTS handle).

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
  │     ├── OllamaTab
  │     └── AboutTab
  │
  ├── /overlay   → Overlay component
  │     ├── BlueWave       (default)
  │     ├── VoiceCard
  │     ├── Waveform
  │     └── Pulse
  │
  └── /history   → History component
```

**State management:**
- `src/stores/config.ts` — reactive `AppConfig` with 400ms debounced auto-save via `save_config` IPC; also listens for `config-changed` events
- `src/stores/status.ts` — live state from `status-tick` events + derived stores (`recording`, `speaking`, `wordCount`, `activeTargetLabel`)

---

## File Locations

| Path | Contents |
|---|---|
| `~/.config/voxctl/config.json` | Main application config |
| `~/.config/voxctl/targets.toml` | Output target definitions |
| `~/.config/voxctl/bindings.toml` | Hotkey binding definitions |
| `~/.local/share/voxctl/models/` | Downloaded Whisper GGUF models |
| `~/.local/share/voxctl/piper-voices/` | Downloaded Piper voice packs |
| `/tmp/voxctl-mcp.sock` | MCP Unix domain socket (Linux) |
| `\\.\pipe\voxctl-mcp` | MCP named pipe (Windows) |
