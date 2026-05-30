# Building VoxCtr on Windows

## Prerequisites

### Required Tools

| Tool | Version | Download |
|---|---|---|
| Rust (via rustup) | 1.75+ | https://rustup.rs/ |
| Node.js | 18+ | https://nodejs.org/ |
| Visual Studio Build Tools | 2019+ | https://visualstudio.microsoft.com/visual-cpp-build-tools/ |
| WebView2 Runtime | Any | Pre-installed on Windows 10 21H2+ and Windows 11 |

### Visual Studio Build Tools

During installation, select the **"Desktop development with C++"** workload. This provides MSVC, the Windows SDK, and the linker required by Rust.

After installation, run builds from a **Visual Studio Developer Command Prompt** or ensure `cl.exe` is on your PATH. The easiest way to ensure this is to install and use `rustup` with the default `stable-x86_64-pc-windows-msvc` toolchain.

### Tauri CLI

```powershell
cargo install tauri-cli
```

### Node dependencies

```powershell
npm install
```

---

## Development Build

```powershell
npm run tauri dev
```

This starts the Vite dev server and compiles the Rust backend in debug mode. Hot-reload is active for Svelte changes; Rust changes require a recompile (~5–30s).

---

## Production Build

### Standard build (no GPU acceleration)

```powershell
npm run tauri build
```

Output artifacts land in `src-tauri\target\release\bundle\`:
- `nsis\VoxCtr_<version>_x64-setup.exe` — NSIS installer
- `msi\VoxCtr_<version>_x64.msi` — MSI package

### Build with CUDA acceleration

If you have an NVIDIA GPU and the CUDA Toolkit installed (11.x or 12.x), you can enable GPU-accelerated inference:

```powershell
# Set CUDA path if needed (adjust to your installed version)
$env:CUDA_PATH = "C:\Program Files\NVIDIA GPU Computing Toolkit\CUDA\v12.3"
$env:CUDA_COMPUTE_CAP = "86"   # Set to your GPU's compute capability

npm run tauri build -- --features cuda
```

Without the `cuda` feature flag, Whisper inference runs on the CPU. The `cuda` feature is opt-in and never required.

### Using the build script

A PowerShell helper script automates prerequisite checks and the build:

```powershell
# Standard build
.\scripts\build_windows.ps1

# With CUDA
.\scripts\build_windows.ps1 -Cuda

# Debug build
.\scripts\build_windows.ps1 -Debug
```

---

## Whisper Models

Place `.bin` model files in `%LOCALAPPDATA%\voxctl\models\` (created on first run).

Download a model manually:

```powershell
$model = "ggml-large-v3.bin"
$url   = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$model"
$dest  = "$env:LOCALAPPDATA\voxctl\models\$model"
New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
Invoke-WebRequest $url -OutFile $dest
```

Supported sizes: `tiny`, `base`, `small`, `medium`, `large-v3`, `large-v3-turbo`.

---

## Piper TTS (optional)

To use Piper neural TTS, download the Windows binary and place it at:

```
%LOCALAPPDATA%\voxctl\piper\piper.exe
```

Download from: https://github.com/rhasspy/piper/releases

Voice models go in `%LOCALAPPDATA%\voxctl\piper-voices\`. The Settings UI has a download button for each supported voice.

---

## Text Injection

On Windows, VoxCtr injects dictated text by:
1. Writing the text to the clipboard via `arboard`
2. Simulating Ctrl+V via PowerShell `SendKeys`

This works in most applications. Native `SendInput` integration is planned to improve reliability and speed.

---

## Code Signing (optional, recommended for distribution)

Without a code signing certificate, Windows SmartScreen will display an "Unknown Publisher" warning when users run the installer.

To sign your build:

1. Obtain an Authenticode certificate (EV certificate eliminates SmartScreen entirely; standard OV certificates reduce warnings after enough users install the app).
2. Set the following environment variables before building:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY    = "path\to\key.pem"
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = "your-passphrase"
```

See the [Tauri signing docs](https://tauri.app/distribute/sign/windows/) for full details.

---

## Troubleshooting

### `error: linker 'link.exe' not found`
MSVC linker is missing. Install Visual Studio Build Tools with the C++ workload, or run `rustup target add x86_64-pc-windows-msvc`.

### `error[E0463]: can't find crate for 'std'`
Wrong target selected. Ensure `rustup default stable-x86_64-pc-windows-msvc`.

### WebView2 missing
Download the WebView2 Evergreen Bootstrapper from Microsoft and run it before launching the app. On Windows 11 and Windows 10 21H2+, WebView2 ships with the OS.

### CUDA build fails
- Confirm `CUDA_PATH` points to an installed CUDA Toolkit.
- Ensure Visual Studio Build Tools are installed (whisper-rs compiles CUDA kernels with MSVC).
- Try without `--features cuda` to rule out a non-CUDA issue first.

### Audio device not detected
VoxCtr uses WASAPI via `cpal`. If no microphone is listed, check Windows privacy settings: **Settings → Privacy & security → Microphone → Allow apps to access your microphone**.
