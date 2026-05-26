# Hotkey System

**Crate:** `crates/voxctr-hotkeys/`

## Overview

VoxCtr listens for global keyboard shortcuts that work regardless of which application has focus. The implementation is platform-specific: **evdev** on Linux and **Win32 hooks** on Windows.

---

## Platform Implementations

### Linux (evdev)

Uses `/dev/input/event*` devices directly, bypassing the desktop environment entirely. This means hotkeys work in:
- X11 sessions
- Wayland sessions
- TTY / no-DE environments
- Over SSH with an active session

**Requirement:** The user must be in the `input` group (or have read access to `/dev/input/event*`):

```bash
sudo usermod -aG input $USER
# Log out and back in
```

The listener opens all event devices that expose keyboard events, watches for key press/release events, and matches against registered bindings.

### Windows

Uses Win32 `SetWindowsHookEx` with `WH_KEYBOARD_LL` to intercept global keyboard events.

No special permissions are required for standard user accounts.

---

## Gesture Recognition

Each binding specifies a `gesture` that controls when recording starts and stops.

### Hold
```
Key Down ──► START RECORDING
Key Up   ──► STOP RECORDING
```
Most natural for short dictations. Recording is exactly as long as the key is held.

### Toggle
```
Key Down (1st) ──► START RECORDING
Key Down (2nd) ──► STOP RECORDING
```
Useful for longer dictations where holding a key would be tiring. Press once to start, press again to stop.

### Double
```
Key Down + Up (rapidly) ──► (ignored)
Key Down + Up (again within threshold) ──► START/STOP TOGGLE
```
Two rapid presses act as a toggle trigger. Distinguishes from accidental single presses.

---

## Key Names

Keys are identified by their **evdev event code name** on Linux. Common keys:

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
| `KEY_ESC` | Escape |
| `KEY_BACKSPACE` | Backspace |
| `KEY_F1`–`KEY_F12` | Function keys |
| `KEY_A`–`KEY_Z` | Letter keys |
| `KEY_0`–`KEY_9` | Number row |

### Finding Key Names

To find the evdev name for any key:
```bash
# Install evtest if not present
sudo apt install evtest

# Run (pick your keyboard device)
sudo evtest /dev/input/event2

# Press the key you want — look for "EV_KEY" lines
# e.g.: Event: type 1 (EV_KEY), code 125 (KEY_LEFTMETA), value 1
```

---

## Hot-Reload

Hotkey bindings can be updated without restarting the listener. When `bindings.toml` changes on disk or bindings are saved via the UI:

1. New bindings are parsed
2. Sent through `binding_reload_tx` channel
3. The listener thread receives them and swaps its internal binding table atomically

This means you can add, modify, or remove hotkeys at runtime.

---

## Multi-Key Combos

`keys` in a binding is an array. VoxCtr tracks which keys from the combo are currently held and only fires the gesture when **all** keys in the combo are pressed:

```toml
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
# Fires only when both Left-Super AND Space are held
```

Order does not matter — holding Meta then Space, or Space then Meta, both trigger.

---

## Conflict Handling

If two bindings share the same key combo, the first matching binding wins. Disable bindings you don't want rather than leaving conflicting ones active:

```toml
[[binding]]
id = "old_binding"
disabled = true
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
# ...
```

---

## GestureEvent

The output of the hotkey system is a stream of `GestureEvent` values sent on `gesture_tx`:

```rust
pub enum GestureEvent {
    StartRecording { binding_id: String, target_ids: Vec<String> },
    StopRecording  { binding_id: String },
}
```

`lib.rs` receives these and coordinates the audio recorder and inference pipeline accordingly.
