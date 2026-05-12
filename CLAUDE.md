# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

VoxCtr is a native, on-device voice-to-text application for Linux with first-class Wayland support. It uses OpenAI's Whisper model for offline transcription and acts as a programmable voice input broker that routes speech to various destinations (keyboard injection, clipboard, shell exec, named pipes, sockets, files, DBus, HTTP webhooks).

## Development Commands

**Run from source:**
```bash
python3 -m venv venv
source venv/bin/activate
pip install -r requirements.txt
./voxctl.sh
```

**Run all tests:**
```bash
python -m pytest tests/ -v
```

**Run a single test file:**
```bash
python -m pytest tests/test_routing_loader.py -v
```

**Run a single test:**
```bash
python -m pytest tests/test_routing_loader.py::TestClass::test_name -v
```

**Install to system (interactive, detects GPU/package manager):**
```bash
bash install.sh
```

**Build AppImage:**
```bash
bash scripts/build_appimage.sh
```

No dedicated linter config exists — project uses standard Python conventions.

## Architecture

The application follows a **pipeline architecture** where audio flows through discrete stages, each in its own thread:

```
Input (evdev hotkey) → Recording (PyAudio) → Transcription (Backend) → Post-processing → Delivery (Router) → [optional: Response FIFO → TTS]
```

### Key Source Modules

| Module | Role |
|---|---|
| `src/main.py` | App orchestration, stage wiring |
| `src/input_listener.py` | evdev hotkey engine; emits gesture + target_id |
| `src/audio_recorder.py` | PyAudio capture with VAD; emits numpy float32 |
| `src/backends/selector.py` | GPU detection (nvidia-smi, sysfs DRM, vulkaninfo) and backend auto-selection |
| `src/backends/protocol.py` | `TranscriptionBackend` / `StreamingTranscriptionBackend` Protocol definitions |
| `src/inference_engine.py` | Transcription orchestration + post-processing |
| `src/routing/router.py` | `OutputTargetRouter` — dispatches text to configured delivery targets |
| `src/routing/models.py` | Core enums/dataclasses: `GestureType`, `HotkeyBinding`, `DeliveryType`, `OutputTarget` |
| `src/routing/loader.py` | TOML load/save for `targets.toml` and `bindings.toml` |
| `src/routing/targets.py` | Concrete delivery implementations (inject, pipe, socket, exec, dbus, file, clipboard) |
| `src/mcp_server.py` | Built-in JSON-RPC MCP server for AI agent control |
| `src/tts_engine.py` | Piper/espeak-ng TTS with voice catalog management |
| `src/tts_responder.py` | FIFO listener that queues agent responses for TTS |
| `src/atspi_context.py` | AT-SPI2 integration for focus tracking and direct text insertion |
| `src/config.py` | JSON config management |
| `src/config_validator.py` | Startup validation of config |
| `src/dbus_service.py` | DBus control interface |

### Transcription Backends

Three pluggable backends implement `TranscriptionBackend` protocol:
- `faster_whisper_backend.py` — NVIDIA CUDA
- `whisper_cpp_backend.py` — AMD/Intel Vulkan, CPU
- `moonshine_backend.py` — ultra-fast CPU-only ONNX

### GUI (`src/gui/`)

PyQt6 tabbed settings dialog, system tray icon, transcription history panel, permissions wizard, and a hot-swappable overlay system. Custom overlays can be dropped into `~/.config/voxctl/overlays/` as Python files.

### Configuration

- Main config: `~/.config/voxctl/config.json` (hierarchical, validated at startup)
- Targets: `~/.config/voxctl/targets.toml` (per-target delivery and post-processing)
- Bindings: `~/.config/voxctl/bindings.toml` (gesture-to-target hotkey mappings)
- See `examples/` for reference TOML configs

### Thread Safety

Audio capture, transcription, post-processing, and text injection all run in separate threads. Communication between stages uses thread-safe queues — avoid shared mutable state across these boundaries.

### Hotkey Gestures

Hold, Toggle, Double-Tap, and Chord gesture types with conflict detection. `DoubleTapMachine` in `src/hotkeys/double_tap.py` is a state machine managing double-tap timing.

### Dependencies Notes

- `pyatspi` (AT-SPI2) is system-only — not on PyPI, installed via distro package manager
- Piper TTS binary installed separately by `install.sh`
- GPU detection is automatic; backends fall back to CPU if no GPU is found
- `noisereduce` is optional for noise suppression
