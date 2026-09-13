# Testing VoxCtrl on Windows

Thanks for helping test this. VoxCtrl is a voice dictation app: you hold a
shortcut, speak, and what you said is typed into whatever window you were using.
Everything runs on your own machine — no audio or text leaves your computer (unless you explicitly configure a Remote Speech Engine).

**Windows support is expanding rapidly.** VoxCtrl now provides both standard CPU and Direct3D 12 GPU-accelerated builds for Windows, along with on-device S1-mini dictation cleanup, neural text-to-speech, and an interactive 7-step onboarding wizard.

This should take about fifteen minutes.

---

## 1. Install

1. Download the installer for your hardware:
   - **[VoxCtrl-windows-x86_64.exe (Standard CPU)](https://github.com/JRufer/VoxCtrl/releases/latest/download/VoxCtrl-windows-x86_64.exe)** — Runs on any modern Windows machine (CPU inference).
   - **[VoxCtrl-windows-x86_64-webgpu.exe (Direct3D 12 GPU)](https://github.com/JRufer/VoxCtrl/releases/latest/download/VoxCtrl-windows-x86_64-webgpu.exe)** — Accelerates Moonshine speech recognition using your GPU (NVIDIA, AMD, Intel) via Direct3D 12.
   - Or visit the **[Latest Release Page](https://github.com/JRufer/VoxCtrl/releases/latest)** to view all release assets and changelogs.
2. Run the installer. Windows will show a blue **"Windows protected your PC"** box, because
   the installer is not yet signed with a certificate. Click **More info**, then
   **Run anyway**.
3. Follow the installer steps, then launch VoxCtrl.

Windows 10 (version 21H2 or newer) or Windows 11. Nothing else to install first.

## 2. First run & Setup Wizard

On first launch, a seven-step setup wizard automatically guides you through:
1. **Welcome** — Overview of setup steps.
2. **Speech Engine** — Choose from 4 transcription engines:
   - `whisper.cpp` (OpenAI Whisper running locally on CPU)
   - `Moonshine` (fast ONNX engine tuned for real-world ambient noise; accelerated on GPU in the WebGPU build)
   - `Parakeet TDT` (NVIDIA FastConformer delivering ultra-fast non-autoregressive transcription)
   - `Remote Speech Engine` (connect to an external or LAN OpenAI-compatible `/v1/audio/transcriptions` server, with live connection testing)
3. **Hotkey** — Bind your preferred global dictation gesture (default: hold **Windows key + Space**).
4. **Overlay HUD** — Select your visual feedback style (Ocean Wave, Voice Card, Waveform, or Pulse Ring).
5. **Test Dictation** — Verify speech capture, inference, and typing into an active window.
6. **Voice (TTS)** — Optionally set up neural text-to-speech feedback (Pocket-TTS, Breeze-TTS-2, VoxCPM2, Inflect-Micro-v2, Piper, or eSpeak-NG).
7. **Done** — Confirmation and diagnostic status.

**If dictation produces nothing at all, check the microphone first.** Windows
denies microphone access silently, with no prompt and no error. Open
**Settings → Privacy & security → Microphone** and make sure "Let desktop apps
access your microphone" is on. This catches most people once.

VoxCtrl keeps running in the system tray after you close its window. Right-click
the tray icon for Settings, quick toggles, or to quit.

---

## 3. What would be most useful to try

The default shortcut is **hold Windows key + Space**, speak, then let go.

### a. Basic Dictation
Open Notepad, hold the shortcut, say a sentence, and release. The text should appear where your cursor is.

### b. Punctuation and Symbols
This is the single most valuable test — the Windows input pipeline synthesizes keystrokes via native `SendInput` Unicode events. Dictate something with complex symbols:

> "fifty percent of users, open paren a plus b close paren, and array bracket zero"

Check characters like `% ( ) + [ ] { } ^ ~` character by character. If any of those come out missing, doubled, or converted into something unexpected, please report it with the exact text you got.

### c. Try Multiple Speech Engines (Settings → Engine)
- **`whisper.cpp`**: Try standard `tiny` or `base` models.
- **`Moonshine`**: Test responsiveness. If using the `webgpu` build, verify GPU acceleration works smoothly.
- **`Parakeet TDT`**: Test non-autoregressive transcription speed.
- **`Remote Speech Engine`**: If you run a local or LAN transcription server (e.g. Faster-Whisper-Server, vLLM, Whisper standalone), test connecting with your custom URL and Bearer token.

### d. Try On-Device S1-mini Dictation Cleanup
In **Settings → Post-Processing**, enable **S1-mini dictation cleanup** (or toggle it per-keybind in **Settings → Hotkeys**). VoxCtrl runs Superwhisper's Qwen3-0.6B model via a Vulkan-accelerated sidecar (with automatic CPU fallback) to normalize raw speech, correct punctuation, and clean spoken self-corrections while strictly preserving voice command triggers. Enabling it greys out the Basic Text Cleanup options in the same tab, since S1-mini covers the same ground.

### e. Try Different Applications
Test dictating into different apps:
- Notepad / text editors
- Web browsers (Chrome, Edge, Firefox)
- VS Code / IDEs
- Windows Terminal / PowerShell / Command Prompt
- Word / Office apps

### f. Watch the Overlay HUD
A floating overlay appears while you speak and plays a smooth spring unload animation when done:
- Does a **black console window** flash or appear? (It shouldn't.)
- Does the overlay **steal focus** from your active window? (It shouldn't.)
- Try the 4 built-in animated styles in **Settings → Visual Feedback**:
  - **Ocean Wave (`blue_wave`)**: Rising tide pool with layered waves and target buoy.
  - **Voice Card (`voice_card`)**: Card flip with holographic sheen and LED dot matrix.
  - **Waveform (`waveform`)**: Oscilloscope CRT power-on trace.
  - **Pulse Ring (`pulse`)**: Radar sweep with target lock reticle.
- Test changing overlay screen position (**Center**, **Top**, or **Bottom**) and multi-monitor selection.
- Test the **Command Overlay Pill** by saying *"VoxCtrl notes, test note"* to see the purple lightning pill appear.

### g. Try Gesture Styles (Settings → Hotkeys)
In **Settings → Hotkeys**, you can configure shortcuts to:
- **Hold** (record while held, transcribe on release)
- **Toggle** (press once to start recording, press again to stop)
- **Double-tap** (double-tap to start, press again to stop)
- **Double-tap & hold** (double-tap and hold second tap)

### h. Long Dictation & Clipboard Fallback
Say a paragraph or two without stopping. Above roughly 2,000 characters, VoxCtrl switches from per-character typing to an atomic clipboard paste, safely restoring your prior clipboard contents afterwards.

### i. Text-to-Speech (TTS) & Memory Modes
In **Settings → TTS**, test voice playback:
- **Pocket-TTS**: Neural voice cloning from reference clips (requires entering a HuggingFace token for gated weights).
- **Breeze-TTS-2**: Voice design from natural-language prompts.
- **VoxCPM2**: Voice design prompts and voice cloning clips.
- **Inflect-Micro-v2**: Compact 38 MB ONNX model.
- **Model Memory Mode**: Test switching between **Always Loaded** and **On Demand** (which drops model weights from memory after 15 minutes of inactivity to conserve RAM). You can also toggle this via the tray menu ("Unload TTS model when idle").

---

## 4. If something goes wrong — one-button Bug Reporting

Open VoxCtrl's settings (right-click the tray icon → Settings) and navigate to **Bug Report** in the sidebar. Describe what happened and click a button. It gathers the log, your Windows version, your CPU and GPU, which build variant you are running, and your settings — with API keys, file paths, username, and dictated text stripped out — and either files it for you (no GitHub account needed) or saves it to a file you can share.

**Before you submit anything, it shows you the entire report.** Click
*"Show me exactly what will be sent"* to inspect the exact payload. What you see is what is sent; there is no fuller hidden version.

### Manual Logs (Alternative)
If you prefer not using the built-in Bug Report tab:
1. **Quit VoxCtrl** (right-click tray icon → Quit).
2. Press **Windows key + R**, paste:
   ```
   %LOCALAPPDATA%\voxctrl
   ```
   and press Enter.
3. Delete **`startup_errors.log`** if present.
4. Start VoxCtrl and reproduce the issue.
5. Quit VoxCtrl and retrieve the newly generated **`startup_errors.log`**.
   The log does **not** contain transcription text or audio data.

---

## 5. Known limitations — expected behavior

These are known characteristics of the Windows platform:

- **Elevated apps.** If a program is running as administrator, Windows blocks non-elevated apps from receiving global hooks and injecting keystrokes into it (e.g. Task Manager, certain installers). Run VoxCtrl as administrator if you need dictation in elevated windows.
- **UAC prompts and Lock Screen.** Windows hides keyboard hooks on the secure desktop, so shortcuts will not fire there.
- **SmartScreen warning.** The installer executable is not yet code-signed with an EV certificate, so Windows SmartScreen warns of an unknown publisher (**More info → Run anyway**).
- **GPU acceleration focus.** The Windows GPU build (`VoxCtrl-windows-x86_64-webgpu.exe`) accelerates the `Moonshine` engine via Direct3D 12 WebGPU and runs `s1-mini` via Vulkan/CPU. `whisper.cpp` and `Parakeet` currently run on CPU on Windows due to an upstream whisper.cpp MSVC static Vulkan registration bug (whisper.cpp #3750).
- **Piper TTS on Windows.** Piper requires manual binary setup on Windows; for seamless out-of-the-box local neural TTS, choose **Pocket-TTS**, **Inflect-Micro-v2**, **VoxCPM2**, or **eSpeak-NG**.
- **Right-hand modifier keys.** A shortcut recorded with Left Ctrl does not fire from Right Ctrl.

---

## 6. Feedback

Any rough edges, layout issues, confusing wording, or unexpected behavior you encounter are valuable feedback. Please file a report through **Settings → Bug Report** or on the [VoxCtrl GitHub Issues page](https://github.com/JRufer/VoxCtrl/issues).

Thank you for testing!
