# Hotkey System

**Crate:** `crates/voxctrl-hotkeys/`

## Overview

VoxCtrl listens for global keyboard shortcuts that work regardless of which application has focus. The implementation is platform-specific: **evdev** on Linux and **Win32 hooks** on Windows.

---

## Platform Implementations

### Linux (evdev)

Uses `/dev/input/event*` devices directly, bypassing the desktop environment entirely. This means hotkeys work in:
- X11 sessions
- Wayland sessions
- TTY / no-DE environments

The evdev keyboard device can be specified via `audio.evdev_device` in config (e.g. `"/dev/input/event4"`). If not set, the listener discovers keyboard devices automatically.

**Requirement:** The user must be in the `input` group (or have read access to `/dev/input/event*`):

```bash
sudo usermod -aG input $USER
# Log out and back in
```

### Windows

Uses Win32 `SetWindowsHookEx` with `WH_KEYBOARD_LL` to intercept global keyboard events. No special permissions are required.

---

## Gesture Recognition

Each binding specifies a `gesture` that controls when recording starts and stops.

### `hold`
```
Key Down ──► START RECORDING (GestureKind::Start)
Key Up   ──► STOP RECORDING  (GestureKind::Stop)
```
Most natural for short dictations. Recording is exactly as long as the key is held.

The `hold_threshold_ms` field (default 200ms) sets the minimum hold duration before a recording start is registered, preventing accidental triggers.

### `toggle`
```
Key Down (1st press) ──► START RECORDING
Key Down (2nd press) ──► STOP RECORDING
```
For longer dictations where holding a key would be tiring. Press once to start, press again to stop.

### `double_tap`
```
Rapid press+release twice within tap_ms ──► START RECORDING
(then behaves as toggle for stop)
```
Two rapid presses trigger recording. The `tap_ms` field (default 250ms) sets the inter-press window. Distinguishes from accidental single presses.

### `chord`
All keys in `keys` must be simultaneously held. Uses the same hold-start / release-stop behavior. Superset-shadowing applies: if another binding's key set is a superset of this one and all its keys are also held, only the longer binding fires.

---

## Key Names

Keys use **evdev event code names** on Linux. On Windows, the same names are mapped to Virtual Key codes internally.

### Modifier Keys
| Name | Key |
|---|---|
| `KEY_LEFTCTRL` | Left Ctrl |
| `KEY_RIGHTCTRL` | Right Ctrl |
| `KEY_LEFTSHIFT` | Left Shift |
| `KEY_RIGHTSHIFT` | Right Shift |
| `KEY_LEFTALT` | Left Alt |
| `KEY_RIGHTALT` | Right Alt / AltGr |
| `KEY_LEFTMETA` | Left Super / Windows key |
| `KEY_RIGHTMETA` | Right Super / Windows key |
| `KEY_CAPSLOCK` | Caps Lock |

### Common Keys
| Name | Key |
|---|---|
| `KEY_SPACE` | Space |
| `KEY_ENTER` | Enter |
| `KEY_TAB` | Tab |
| `KEY_ESCAPE` | Escape |
| `KEY_BACKSPACE` | Backspace |
| `KEY_F1`–`KEY_F12` | Function keys |
| `KEY_A`–`KEY_Z` | Letter keys |
| `KEY_0`–`KEY_9` | Number row |

### Finding Key Names

To discover the evdev name for any key:
```bash
# Install evtest if not present
sudo apt install evtest

# Run (pick your keyboard device)
sudo evtest /dev/input/event2

# Press the key — look for "EV_KEY" lines:
# Event: type 1 (EV_KEY), code 125 (KEY_LEFTMETA), value 1
```

---

## Hot-Reload

Hotkey bindings update at runtime without restarting the listener. When bindings are saved via the UI or `save_bindings` IPC command:

1. New bindings are written to `bindings.toml`
2. Sent through the `hotkey_reloader` crossbeam channel
3. The listener thread receives them and swaps its binding state table

---

## Multi-Key Combos

`keys` is an array. All keys must be pressed for the gesture to activate. Order doesn't matter:

```toml
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
# Fires when both Left-Super AND Space are held, in any order
```

---

## GestureEvent

The output of the hotkey system is a stream of `GestureEvent` values sent on the `gesture_tx` channel:

```rust
pub struct GestureEvent {
    pub binding_id: String,
    pub binding_label: String,
    pub target_id: String,  // comma-joined for multi-target
    pub kind: GestureKind,
}

pub enum GestureKind {
    Start,  // Begin recording
    Stop,   // End recording
}
```

`lib.rs` receives these and coordinates the audio recorder and inference pipeline.

---

## Conflict Handling

If two bindings share the same key combo, the first matching binding wins (the one listed first in `bindings.toml`). Disable unused bindings rather than leaving conflicts active:

```toml
[[binding]]
id = "old_binding"
disabled = true
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
# ...
```
