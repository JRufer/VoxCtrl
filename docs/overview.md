# Overview

## What is VoxCtrl?

VoxCtrl ("Voice Controller") is a desktop dictation application that turns your voice into text and routes it wherever you need it — injected directly into the focused window, saved to a file, sent to an HTTP endpoint, or handed off to an LLM agent via the MCP protocol.

It is designed as a **programmable voice input broker**: you define output commands (where text goes) and hotkey bindings (what keys trigger recording for which commands), and VoxCtrl handles the rest. A command can also be picked mid-sentence by saying "VoxCtrl" and its name.

By default, everything runs locally on-device. You can also choose the **Remote Speech Engine** ("Bring Your Own Voice Engine") to offload speech recognition to a dedicated server on your local network or a cloud API.

---

## Key Features

### Core Dictation
- **Four speech recognition engines**:
  - **`whisper.cpp`** — OpenAI Whisper running entirely on-device with multi-threaded CPU or Vulkan/CUDA GPU acceleration (tiny through large-v3).
  - **`Moonshine`** — On-device ONNX speech recognition tuned for noisy rooms and conversational audio.
  - **`Parakeet TDT`** — Ultra-fast non-autoregressive FastConformer transcription with zero repetition loops.
  - **`Remote Speech Engine` ("Bring Your Own Voice Engine")** — Offload transcription to any OpenAI-compatible `/v1/audio/transcriptions` server (Faster-Whisper-Server, vLLM, LocalAI, or cloud APIs) with **0 MB local RAM/VRAM footprint**.
- **Hold-to-record, toggle, double-tap, or double-tap & hold** gesture modes per hotkey binding.
- **Global shortcuts without keyboard access** — registered with your desktop through the XDG `GlobalShortcuts` portal, so VoxCtrl is told only when its own shortcut fires and never reads a keystroke ([details](privacy.md)).

### First-Run Setup
A seven-step wizard runs the first time VoxCtrl starts on a machine with no config file, covering the choices the app cannot make for you: speech engine (`whisper.cpp`, `Moonshine`, `Parakeet TDT`, or `Remote Speech Engine` with live connection testing and model discovery), model size, hotkey gesture and key combination, on-screen overlay, a live end-to-end dictation test, and optional speech output. Choices are written to the config as they are made rather than at the end, so a wizard that is quit halfway still leaves the app configured as far as it got. The final screen reports anything that failed — a model that would not download, a shortcut the desktop refused — with the underlying error, rather than claiming the app is ready.

### Privacy & Offline Operation
- Zero network requests during normal operation for all on-device backends (`whisper.cpp`, `Moonshine`, `Parakeet TDT`).
- Audio only leaves your machine if you explicitly configure a **Remote Speech Engine** pointing to a LAN or remote server.
- No analytics, crash reporting, or telemetry.
- All local models and voices stored under `~/.local/share/voxctrl/`.

### Flexible Output Routing
Eleven output delivery types:

| Type | What it does |
|---|---|
| `inject` | Simulates keystrokes into the focused window (wtype on Wayland, xdotool on X11, native SendInput on Windows) |
| `clipboard` | Copies text to the system clipboard |
| `exec` | Runs a shell command with the text as an argument |
| `pipe` | Writes to a named FIFO pipe |
| `socket` | Sends over a Unix domain socket or TCP connection |
| `file` | Appends to a file with optional timestamp prefix |
| `dbus` | Emits a DBus signal on the session bus |
| `http` | POSTs to an HTTP endpoint |
| `webhook` | POSTs with HMAC-SHA256 signed payload |
| `speak` | Speaks transcribed text aloud using the active TTS engine |
| `chat` | Conversational multi-turn chat with an OpenAI-compatible LLM endpoint |

A single hotkey binding can route to **multiple targets simultaneously**.

### Visualization & HUD
- Transparent floating overlay window with real-time audio visualization rendered natively via Slint helper (`voxctrl-overlay`)
- Four primary animated styles: Ocean Wave (default), Voice Card, Waveform, and Pulse Ring (plus Mono Bars, Neon Spectrum, Retro Terminal, Analog VU)
- Spring-driven load and unload animations with audio-reactive geometry
- Voice Command Trigger overlay pill with lightning badge and payload preview
- Auto-show on recording start, auto-hide on completion

### Post-Processing Pipeline
Applied after transcription before delivery:
- Filler word removal (`um`, `uh`, `hmm`, `er`, `ah`, `ugh`, `mhm`)
- Spoken punctuation conversion (`"period"` → `.`, `"comma"` → `,`, and 20+ more)
- Auto-format lists (detects "first/second/third" ordinals → numbered list)
- Snippet expansion (custom shorthand → full text)
- Custom vocabulary fuzzy correction (Levenshtein matching for proper nouns/domain terms)
- Code mode (camelCase conversion, spoken operators)
- **On-device S1-mini dictation cleanup** (Superwhisper Qwen3-0.6B model running via Vulkan-accelerated `voxctrl-llm-sidecar` for offline text normalization and punctuation correction with CPU fallback)
- Optional LLM rewrite via any OpenAI-compatible API server (clean, formal, casual, bullet, concise, or custom prompt)

### Text-to-Speech
- Six local and neural TTS engines: Breeze-TTS-2 (voice design prompts), Piper (neural ONNX voices), Pocket-TTS (voice cloning from audio clips), Inflect-Micro-v2 (compact 38 MB ONNX), VoxCPM2 (voice design & cloning), and eSpeak-NG (lightweight fallback)
- **On-demand model memory mode**: unloads large neural weights after 15 minutes of inactivity to conserve RAM
- Pronunciation snippet expansion dictionary
- `speak_text` callable from hotkeys, MCP, or routing targets

### LLM Integration (MCP Server)
VoxCtrl exposes a Model Context Protocol server so LLM agents (Claude Desktop, Cursor, etc.) can:
- Trigger voice recording and receive the transcription
- Queue TTS playback
- Query live recording/speaking status

### DBus Service (Linux)
Exposes `ai.voxctrl.Dictation` on the session bus for shell scripts and desktop integrations to start/stop recording and receive text output as signals.

---

## Design Principles

**Local-first.** The app functions identically offline. All models download once and run locally forever after.

**Composable routing.** Targets and bindings are data files (TOML), not hardcoded behavior. You can change where your voice goes without touching the app.

**Hot-reloadable config.** The app watches its config files. Edit `targets.toml` or `config.json` in your editor and VoxCtrl picks up changes instantly.

**Minimal footprint.** No Electron, no Node runtime. Tauri gives you a native WebView shell around a compiled Rust backend. The installed binary is ~tens of MB vs ~hundreds for Electron equivalents.

**Low latency.** Audio is captured on a dedicated thread. Inference runs on a dedicated thread. UI updates and delivery happen concurrently via async channels. Hold a key and you're recording within milliseconds.
