# Building VoxCtr (Rust + Tauri)

This document covers building the Rust/Tauri port of VoxCtr from source on
**Linux** and **Windows**. The Python source in `src/` is the legacy
implementation; the Rust port lives in `rust/`.

---

## Table of Contents

1. [Prerequisites — all platforms](#1-prerequisites--all-platforms)
2. [Linux prerequisites](#2-linux-prerequisites)
3. [Windows prerequisites](#3-windows-prerequisites)
4. [First-time setup](#4-first-time-setup)
5. [Development build](#5-development-build)
6. [Release build](#6-release-build)
7. [Packaging](#7-packaging)
8. [Whisper model download](#8-whisper-model-download)
9. [Optional features](#9-optional-features)
10. [Troubleshooting](#10-troubleshooting)

---

## 1. Prerequisites — All Platforms

### Rust toolchain
Install via [rustup](https://rustup.rs/):
```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup update stable
```
Required Rust version: **1.77+** (uses workspace package inheritance).

### Node.js + npm
Required for the Svelte frontend.  
Install via [https://nodejs.org](https://nodejs.org) or your system package manager.  
Required version: **Node.js 20+**, **npm 10+**.

### Tauri CLI
```sh
cargo install tauri-cli --version "^2" --locked
```
Or via npm (pick one):
```sh
npm install --save-dev @tauri-apps/cli@^2
```

---

## 2. Linux Prerequisites

### System libraries
**Debian / Ubuntu:**
```sh
sudo apt update
sudo apt install -y \
  libwebkit2gtk-4.1-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf \
  pkg-config \
  build-essential \
  libgtk-3-dev \
  libglib2.0-dev \
  libdbus-1-dev \
  libportaudio2 \
  portaudio19-dev
```

**Fedora / RHEL:**
```sh
sudo dnf install -y \
  webkit2gtk4.1-devel \
  openssl-devel \
  libayatana-appindicator-devel \
  librsvg2-devel \
  patchelf \
  pkg-config \
  gcc \
  portaudio-devel \
  dbus-devel
```

**Arch Linux:**
```sh
sudo pacman -S --needed \
  webkit2gtk-4.1 \
  openssl \
  libayatana-appindicator \
  librsvg \
  patchelf \
  pkgconf \
  base-devel \
  portaudio \
  dbus
```

### Text injection tools (at least one required)
```sh
# Wayland
sudo apt install -y wtype wl-clipboard

# X11 / XWayland
sudo apt install -y xdotool xclip
```

### Audio playback (for TTS)
```sh
sudo apt install -y alsa-utils   # provides aplay
# or
sudo apt install -y pulseaudio-utils
```

### Optional: whisper-cli binary
If you do **not** build with the `whisper-bundled` feature, you need the
`whisper-cli` binary on your PATH. Build it from source:
```sh
git clone https://github.com/ggerganov/whisper.cpp
cd whisper.cpp
mkdir build && cd build
cmake .. -DWHISPER_BUILD_EXAMPLES=ON
cmake --build . --config Release -j$(nproc)
sudo install -m755 bin/whisper-cli /usr/local/bin/
```

### Optional: Piper TTS binary
```sh
# Download and extract the prebuilt binary
mkdir -p ~/.local/share/voxctl/piper
wget -O /tmp/piper.tar.gz \
  https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_linux_x86_64.tar.gz
tar -xf /tmp/piper.tar.gz -C ~/.local/share/voxctl/
```

### Input device permissions (for evdev hotkeys)
```sh
sudo usermod -aG input $USER
# Log out and back in for the group to take effect.
```

---

## 3. Windows Prerequisites

### Visual Studio Build Tools
Download from https://visualstudio.microsoft.com/visual-cpp-build-tools/  
Select the workload: **"Desktop development with C++"**

Or install via winget:
```powershell
winget install Microsoft.VisualStudio.2022.BuildTools `
  --override "--add Microsoft.VisualStudio.Workload.VcTools"
```

### WebView2 Runtime
Included in Windows 10 (1803+) and Windows 11 by default.  
If missing (e.g., LTSC editions):
```powershell
winget install Microsoft.EdgeWebView2Runtime
```

### Optional: whisper-cli (Windows)
Build whisper.cpp with CMake and MSVC:
```powershell
git clone https://github.com/ggerganov/whisper.cpp
cd whisper.cpp
cmake -B build -DWHISPER_BUILD_EXAMPLES=ON
cmake --build build --config Release
# Copy build\bin\Release\whisper-cli.exe to somewhere on %PATH%
```

### Optional: Piper TTS (Windows)
Download `piper_windows_amd64.zip` from  
https://github.com/rhasspy/piper/releases and extract to:
```
%LOCALAPPDATA%\voxctl\piper\piper.exe
```

---

## 4. First-time Setup

```sh
# Clone (or open) the repo
cd VoxCtr/rust

# Install frontend dependencies
npm install

# (Optional) Verify the Rust workspace compiles
cargo check --workspace
```

---

## 5. Development Build

Launches a hot-reloading Tauri window with Vite's dev server for the
Svelte frontend. Rust backend is rebuilt on change via `cargo`.

```sh
cd VoxCtr/rust

# Start dev mode (frontend + Tauri)
cargo tauri dev
```

Or, if using the npm script:
```sh
npm run tauri dev
```

What happens:
- Vite starts on `http://localhost:5173`
- `tauri-cli` builds the Rust backend in debug mode
- A Tauri window opens pointing at the Vite dev server
- Frontend changes hot-reload instantly
- Rust changes trigger a backend rebuild and window reload

### Dev environment variables
| Variable | Purpose |
|---|---|
| `RUST_LOG=voxctr=debug` | Verbose backend logging |
| `TAURI_ENV_DEBUG=1` | Enables source maps in the frontend build |

```sh
RUST_LOG=voxctr=debug cargo tauri dev
```

---

## 6. Release Build

Produces an optimised native binary with the frontend bundled.

```sh
cd VoxCtr/rust

# Build frontend first
npm run build

# Build Rust in release mode
cargo tauri build
```

The output is in:
- **Linux:** `rust/target/release/voxctr` (binary) and
  `rust/target/release/bundle/` (deb, AppImage)
- **Windows:** `rust/target\release\voxctr.exe` and
  `rust\target\release\bundle\` (msi, nsis installer)

### Cross-compilation
Cross-compilation is not officially supported by Tauri. Build on each
target platform natively. For CI, use GitHub Actions with separate
`ubuntu-latest` and `windows-latest` runners.

---

## 7. Packaging

### Linux — AppImage
```sh
cargo tauri build --bundles appimage
# Output: rust/target/release/bundle/appimage/VoxCtr_0.1.0_amd64.AppImage
```

### Linux — Debian package
```sh
cargo tauri build --bundles deb
# Output: rust/target/release/bundle/deb/voxctr_0.1.0_amd64.deb
```

### Windows — NSIS installer
```powershell
cargo tauri build --bundles nsis
# Output: rust\target\release\bundle\nsis\VoxCtr_0.1.0_x64-setup.exe
```

### Windows — MSI
```powershell
cargo tauri build --bundles msi
# Output: rust\target\release\bundle\msi\VoxCtr_0.1.0_x64_en-US.msi
```

---

## 8. Whisper Model Download

VoxCtr uses GGUF-format models compatible with whisper.cpp.

### Default model directory
| Platform | Path |
|---|---|
| Linux | `~/.local/share/voxctl/models/` |
| Windows | `%LOCALAPPDATA%\voxctl\models\` |

### Download a model
```sh
# Recommended starting model (good quality, fast on CPU)
mkdir -p ~/.local/share/voxctl/models
wget -P ~/.local/share/voxctl/models/ \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin

# Better quality (requires more RAM/VRAM)
wget -P ~/.local/share/voxctl/models/ \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin
```

### Model size guide
| Model | RAM | Quality | Speed (CPU) |
|---|---|---|---|
| tiny | ~75 MB | Low | Very fast |
| base | ~150 MB | OK | Fast |
| small | ~500 MB | Good | Moderate |
| medium | ~1.5 GB | Better | Slow |
| large-v3 | ~3 GB | Best | Very slow |
| large-v3-turbo | ~1.6 GB | Best | Moderate |

Then set the model in Settings → Engine → Model size.

---

## 9. Optional Features

### `whisper-bundled` — statically link whisper.cpp

Builds whisper.cpp from source inside the Rust build (no external
`whisper-cli` binary required). Requires CMake and a C++ compiler.

```sh
# Linux
sudo apt install -y cmake

# Build with bundled whisper
cargo tauri build --features voxctr-inference/whisper-bundled
```

In `rust/crates/voxctr-inference/Cargo.toml`, change the default:
```toml
[features]
default = ["whisper-bundled"]
```

### `noisereduce` — RNNoise noise suppression

```sh
cargo tauri build --features voxctr-audio/noisereduce
```

### `moonshine` — Moonshine ONNX backend

Requires the ONNX Runtime shared library.

```sh
# Download onnxruntime (Linux example)
wget https://github.com/microsoft/onnxruntime/releases/download/v1.19.2/\
onnxruntime-linux-x64-1.19.2.tgz
tar xf onnxruntime-linux-x64-1.19.2.tgz
export ORT_DYLIB_PATH=$PWD/onnxruntime-linux-x64-1.19.2/lib/libonnxruntime.so

cargo tauri build --features voxctr-inference/moonshine
```

---

## 10. Troubleshooting

### `webkit2gtk` not found (Linux)
```
error: Package 'webkit2gtk-4.1' not found
```
Make sure you installed `libwebkit2gtk-4.1-dev`. Some older distros only
have webkit2gtk-4.0; upgrade your distro or build webkit2gtk from source.

### `Cannot find -lportaudio` (Linux)
```sh
sudo apt install portaudio19-dev
```

### `evdev` permission denied (Linux)
```
PermissionError: [Errno 13] Permission denied: '/dev/input/event4'
```
Add yourself to the `input` group:
```sh
sudo usermod -aG input $USER
newgrp input   # or log out and back in
```

### Overlay window not transparent (Linux)
Transparency requires a compositor. Enable one for your desktop:
- **KDE Plasma:** Settings → Display & Monitor → Compositor → Enable
- **GNOME:** Mutter handles compositing by default
- **i3/Sway:** use picom or similar

### whisper-cli not found
Either install whisper.cpp (see §2) or set the binary path in Settings →
Engine → whisper-cli binary path to an absolute path.

### Windows: `WebView2 not installed`
```powershell
winget install Microsoft.EdgeWebView2Runtime
```

### Windows: Hotkeys not captured
The `rdev` global hook requires admin rights on some Windows versions, or
may conflict with other hotkey managers (e.g., AutoHotkey). Try running
VoxCtr as administrator for testing.

### `RUST_LOG` for debugging
```sh
# Very verbose
RUST_LOG=voxctr=trace cargo tauri dev

# Only warnings and errors
RUST_LOG=warn cargo tauri dev
```

---

## Architecture Notes

```
rust/
├── Cargo.toml                  # Workspace root
├── package.json                # Frontend npm config (Svelte + Tauri)
├── vite.config.ts              # Vite bundler config
├── svelte.config.js
├── index.html                  # Frontend entry point
├── src/                        # Svelte 5 frontend
│   ├── main.ts
│   ├── App.svelte              # Routes to Settings / Overlay / History
│   ├── stores/
│   │   ├── status.ts           # Recording / speaking / word count
│   │   └── config.ts           # Full app config (synced with Rust)
│   └── lib/
│       ├── Settings/           # Settings window (8 tabs)
│       ├── Overlay/            # Recording indicator overlays
│       └── History/            # Transcript history viewer
├── src-tauri/                  # Tauri Rust backend
│   ├── Cargo.toml
│   ├── tauri.conf.json         # Window layout, bundle config
│   ├── capabilities/
│   │   └── default.json        # Tauri permission grants
│   └── src/
│       ├── main.rs             # Binary entry (starts Tokio runtime)
│       ├── lib.rs              # App wiring (audio pipeline, tray, windows)
│       ├── commands.rs         # Tauri commands exposed to frontend
│       └── state.rs            # Shared AppState (recording flags, history)
└── crates/
    ├── voxctr-config/          # Config types, load/save, validation
    ├── voxctr-routing/         # 9 delivery types, TOML loader, router
    ├── voxctr-hotkeys/         # evdev (Linux) / rdev (Windows) + gestures
    ├── voxctr-audio/           # CPAL capture, resampling
    ├── voxctr-inference/       # Whisper.cpp backend, post-processing
    ├── voxctr-tts/             # Piper + eSpeak, voice download, FIFO listener
    ├── voxctr-inject/          # wtype / xdotool / clipboard injection
    ├── voxctr-dbus/            # zbus DBus service (Linux only)
    ├── voxctr-mcp/             # JSON-RPC MCP server (Unix socket / named pipe)
    └── voxctr-llm/             # Ollama HTTP client + post-processing modes
```

The audio pipeline runs as a set of OS threads connected by
`crossbeam_channel` queues:

```
[evdev/rdev hotkey thread]
         │ GestureEvent
         ▼
[App coordinator (Tokio)]
         │ start/stop flag
         ▼
[CPAL audio thread] ──AudioChunk──► [Inference thread (whisper.cpp)]
                                             │ InferenceOutput
                                             ▼
                                    [Tokio task: inject + route + history]
                                             │
                                    ┌────────┴────────┐
                                 [wtype/xdotool]   [Router → targets]
```

The Svelte frontend communicates with the Rust backend exclusively via
Tauri commands (`invoke`) and events (`listen`). No direct system calls
are made from the frontend.
