## Downloads

Pick the build for your platform and GPU:

| Platform | File suffix | GPU required |
|---|---|---|
| Linux | `-linux-x86_64-vulkan.AppImage` | None — GPU used if present |
| Linux (newer distros) | `-linux-x86_64-vulkan-webgpu.AppImage` | None — GPU used if present; also accelerates Moonshine and Parakeet. Needs glibc 2.38+ (Ubuntu 24.04, Fedora 39, Debian 13, Arch and similar) |
| Windows | `-windows-x86_64-webgpu.exe` | None — GPU used if present |

**There is one Windows build.** It accelerates the **Moonshine** and
**Parakeet** speech engines on any Direct3D 12 GPU — NVIDIA, AMD or
Intel — through ONNX Runtime's WebGPU provider, and runs on the CPU
when no usable GPU is found, so it replaces the separate CPU
installer rather than sitting beside it. Each engine has its own
**GPU acceleration** checkbox in **Settings → Engine**, and the chip
beside the engine's name says which device it is using. Whisper.cpp still runs on the CPU on Windows either
way, because its only GPU path here is ggml's Vulkan backend, which
does not register on Windows MSVC static builds. If your GPU is not
usable the app says so in the log and falls back to the CPU rather
than pretending. If you were running the separate CPU installer,
VoxCtrl's own updater will move you onto this one; nothing to do.

**Linux has two builds.** The Vulkan AppImage accelerates
any NVIDIA/AMD/Intel GPU through the host Vulkan driver and runs on
the CPU when there is no Vulkan device, so it replaces the separate
CPU AppImage rather than sitting beside it — the same file is right
whether or not you have a GPU. The Vulkan + WebGPU AppImage is the same
app plus GPU acceleration for Moonshine and Parakeet; it bundles Dawn, which
needs glibc 2.38 or newer, so it will not start on Ubuntu 22.04 or Debian 12 —
use the plain Vulkan AppImage there. VoxCtrl's updater keeps each install on
the variant it is running. If you were running the old CPU AppImage, the
updater moves you onto the Vulkan one; nothing to do.

To keep an engine on the CPU regardless, untick **GPU acceleration** for it
in **Settings → Engine**. Worth doing if your only Vulkan device is a software
rasterizer such as Mesa's lavapipe, which is slower than the CPU
backend.

The Linux `.deb` now requires `libvulkan1`, which the package
manager pulls in for you.

**The Windows build** is an NSIS installer, not code-signed yet, so
SmartScreen will warn that the publisher is unknown: choose
**More info → Run anyway**.

### Windows — first run

Windows 10 21H2 or newer, or Windows 11. Everything VoxCtrl needs is
in the installer; WebView2 ships with Windows 11 and the installer
fetches it on older builds.

Two things are worth knowing before the first dictation:

- **Microphone access.** Windows denies it silently. If dictation
  produces nothing, check **Settings → Privacy & security →
  Microphone** and allow desktop apps.
- **Global shortcuts** arrive through a Windows keyboard hook, so
  every gesture works — but Windows does not deliver keys to it
  while an elevated application has focus, or on the UAC prompt and
  lock screen. Shortcuts do not fire there, and dictated text cannot
  be typed into an elevated window unless VoxCtrl is elevated too.

### Linux — first run

The Vulkan AppImage needs glibc 2.35 or newer (Ubuntu 22.04+, Linux Mint 21+,
Debian 12+, Fedora 36+, Arch); the Vulkan + WebGPU one needs 2.38 or newer.
Nothing has to be installed first — not even `libfuse2`.

```bash
# The file you downloaded: -vulkan.AppImage or -vulkan-webgpu.AppImage
APP=./VoxCtrl-linux-x86_64-vulkan.AppImage
chmod +x "$APP"
# Optional: installs the desktop entry and text-injection helpers.
# Global shortcuts need no permissions — your desktop handles them.
"$APP" --install
# Launch the application
"$APP"
```
