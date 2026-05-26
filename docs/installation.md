# Installation & Setup

## System Requirements

### Linux
- **OS:** Any modern Linux distro (Ubuntu 20.04+, Fedora 36+, Arch, etc.)
- **Display:** X11 or Wayland
- **Audio:** PulseAudio or PipeWire (ALSA fallback supported)
- **Required packages:** `libwebkit2gtk-4.1`, `libayatana-appindicator3` or `libappindicator3`
- **Optional:** `wtype` (Wayland injection), `xdotool` (X11 injection), `espeak-ng` (TTS fallback)

### Windows
- **OS:** Windows 10 (1903+) or Windows 11
- **Runtime:** WebView2 (pre-installed on Win11; auto-downloadable on Win10)

---

## Installing the AppImage (Linux)

The recommended distribution format is an AppImage — a single portable executable with all dependencies bundled.

```bash
# Download the latest AppImage
curl -LO https://github.com/jrufer/voxctr/releases/latest/download/VoxCtr.AppImage

# Make executable
chmod +x VoxCtr.AppImage

# Run
./VoxCtr.AppImage
```

Or use the install script for system integration (desktop entry, udev rules):
```bash
bash install.sh
```

The install script:
1. Copies the AppImage to `~/.local/bin/voxctr`
2. Installs a `.desktop` file for application launchers
3. Creates udev rules for `/dev/input` access (required for global hotkeys)
4. Adds the current user to the `input` group

> **Note:** After running `install.sh`, you must log out and back in for the `input` group membership to take effect. Until then, global hotkeys will not work.

---

## Permissions Setup (Linux)

### Global Hotkeys
VoxCtr uses evdev to listen for global keyboard events. Your user must be in the `input` group:

```bash
sudo usermod -aG input $USER
# Log out and back in
```

Verify:
```bash
groups $USER | grep input
```

### Wayland Text Injection
For Wayland sessions, install `wtype`:
```bash
# Ubuntu/Debian
sudo apt install wtype

# Arch
sudo pacman -S wtype

# Fedora
sudo dnf install wtype
```

### X11 Text Injection
For X11 sessions, install `xdotool`:
```bash
# Ubuntu/Debian
sudo apt install xdotool

# Arch
sudo pacman -S xdotool
```

---

## First Run

On first launch, VoxCtr will:
1. Create `~/.config/voxctl/` with default `config.json`, `targets.toml`, and `bindings.toml`
2. Create `~/.local/share/voxctl/` for model and voice storage
3. Open the Settings window

### Download a Whisper Model
Before you can dictate, you need a speech recognition model:
1. Go to Settings → Engine
2. Choose a model size (recommendation: `base` for speed, `small` for better accuracy)
3. Click "Download" and wait for completion (~100 MB for `base`)

### Configure a Hotkey
A default binding (`Super + Space`, hold gesture → inject to focused window) is created automatically. Verify it in Settings → Hotkeys, or change the key combo if it conflicts with your desktop environment.

### Test Dictation
1. Open any text editor
2. Click into the text area
3. Hold `Super + Space` and speak
4. Release to transcribe

---

## Optional Setup

### GPU Acceleration
If you have an NVIDIA GPU, install the CUDA toolkit and set `engine.device = "cuda"` in config. VoxCtr will auto-detect it with `"auto"`.

For AMD/Intel GPUs, Vulkan support (`engine.device = "vulkan"`) requires:
```bash
# Ubuntu
sudo apt install vulkan-tools libvulkan1

# Arch
sudo pacman -S vulkan-icd-loader
```

### Ollama Post-Processing
If you want LLM grammar correction:
1. Install [Ollama](https://ollama.ai/)
2. Pull a model: `ollama pull llama3.2`
3. Enable in Settings → Ollama

### MCP Server (Claude Desktop / Cursor)
1. Enable in Settings → Engine → MCP Server
2. Configure your MCP client to connect to `/tmp/voxctl-mcp.sock`

---

## Building from Source

See [Development Guide](./development.md).

---

## Troubleshooting

### Hotkeys not working
- Check you are in the `input` group: `groups | grep input`
- Log out and back in after adding to group
- On some distros, the udev rule path differs — check `install.sh` for details

### No audio devices found
- Run `arecord -l` to verify your mic is recognized by ALSA
- Check if PulseAudio/PipeWire is running: `pactl info`
- Try setting `audio.device_index` manually to a specific device index

### Text not injecting on Wayland
- Verify `wtype` is installed: `which wtype`
- Some applications block synthetic input (e.g. terminals with certain settings)
- Clipboard fallback always works — use `delivery = "clipboard"` as a workaround

### Whisper outputs wrong language
- Set `engine.language` to your language code (e.g. `"de"`, `"fr"`, `"es"`)
- Use a larger model for better non-English accuracy

### AppImage won't launch
- Install FUSE: `sudo apt install fuse libfuse2`
- Or extract and run directly: `./VoxCtr.AppImage --appimage-extract && squashfs-root/AppRun`
