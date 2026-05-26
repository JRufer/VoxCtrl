# Output Routing

**Crate:** `crates/voxctr-routing/`

## Overview

VoxCtr's routing system decouples *what you say* from *where it goes*. You define:

- **Output Targets** (`targets.toml`) — named delivery destinations
- **Hotkey Bindings** (`bindings.toml`) — which keys trigger which targets

Both files are hot-reloaded when changed on disk.

---

## Output Targets

Defined in `~/.config/voxctl/targets.toml`. Each `[[target]]` block describes one destination.

### Common Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | Yes | Unique identifier, referenced by bindings |
| `label` | string | Yes | Display name in the UI |
| `delivery` | string | Yes | Delivery type (see below) |
| `append_newline` | bool | No (false) | Append `\n` after injected text |
| `send_on_release` | bool | No (false) | Wait for hotkey release before delivering |
| `initial_prompt` | string | No | Whisper context prompt override for this target |
| `processing` | object | No | Per-target post-processing overrides |

### Delivery Types

#### `inject` — Keystroke Injection
Simulates typing into the currently focused window.

```toml
[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
append_newline = false
```

Linux injection priority:
1. `wtype` (Wayland)
2. `xdotool type --clearmodifiers` (X11)
3. Clipboard + Ctrl+V fallback

Windows: clipboard paste via PowerShell.

---

#### `clipboard` — System Clipboard
Copies text to clipboard. Does not paste.

```toml
[[target]]
id = "clipboard"
label = "Copy to Clipboard"
delivery = "clipboard"
```

---

#### `file` — File Append
Appends text to a file on disk.

```toml
[[target]]
id = "notes"
label = "Meeting Notes"
delivery = "file"
file_path = "~/Documents/notes.md"
file_prefix = "- "        # Prepend to each entry
file_timestamp = true     # Prepend ISO timestamp
```

Additional fields:

| Field | Type | Description |
|---|---|---|
| `file_path` | string | Path to the file (supports `~`) |
| `file_prefix` | string | String prepended to each entry |
| `file_timestamp` | bool | Prepend `[2024-01-15 10:23:45]` timestamp |

---

#### `exec` — Shell Command
Runs a command with the transcribed text passed as an argument or via stdin.

```toml
[[target]]
id = "cmd"
label = "Custom Script"
delivery = "exec"
exec_command = "/home/user/scripts/handle-voice.sh"
# Text is passed as $1
```

---

#### `http` — HTTP POST
POSTs the text as a JSON body to an HTTP endpoint.

```toml
[[target]]
id = "api"
label = "My API"
delivery = "http"
http_url = "http://localhost:8080/voice"
```

Request body:
```json
{"text": "transcribed text here"}
```

---

#### `webhook` — Signed HTTP POST
Same as `http` but adds HMAC-SHA256 signature header for verification.

```toml
[[target]]
id = "secure_hook"
label = "Signed Webhook"
delivery = "webhook"
http_url = "https://example.com/hook"
webhook_secret = "your-shared-secret"
```

Adds header: `X-VoxCtr-Signature: sha256=<hex>`

---

#### `socket` — Unix Domain Socket
Sends text (newline-terminated) to a Unix socket.

```toml
[[target]]
id = "sock"
label = "Socket Output"
delivery = "socket"
socket_path = "/tmp/myapp.sock"
```

---

#### `pipe` — Named FIFO
Writes to a named FIFO pipe.

```toml
[[target]]
id = "fifo"
label = "FIFO Output"
delivery = "pipe"
pipe_path = "/tmp/voice.fifo"
```

---

#### `dbus` — DBus Signal
Emits the text as a DBus signal on the `ai.voxctl.Dictation` interface.

```toml
[[target]]
id = "dbus"
label = "DBus Output"
delivery = "dbus"
```

---

#### `mcp` — MCP Response Queue
Enqueues the text as a response to a pending MCP `transcribe_voice` tool call.

This is used internally by the MCP server — you don't typically configure this in targets.toml.

---

### Per-Target Processing

Each target can override global post-processing settings:

```toml
[[target]]
id = "code_editor"
label = "Code Editor"
delivery = "inject"

[target.processing]
code_mode = true          # Preserve programming syntax
remove_fillers = false    # Don't strip fillers
ollama_enabled = false    # Skip LLM rewrite
```

---

## Hotkey Bindings

Defined in `~/.config/voxctl/bindings.toml`. Each `[[binding]]` block maps a key combo + gesture to one or more targets.

### Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | Yes | Unique identifier |
| `label` | string | Yes | Display name in UI |
| `keys` | string[] | Yes | Key names (evdev format on Linux) |
| `gesture` | string | Yes | `"hold"`, `"toggle"`, or `"double"` |
| `target_id` | string | No | Single target (deprecated, use `target_ids`) |
| `target_ids` | string[] | Yes | Ordered list of targets to route to |
| `disabled` | bool | No (false) | Disable without removing |

### Gesture Types

| Gesture | Behavior |
|---|---|
| `hold` | Recording starts on press, stops on release |
| `toggle` | First press starts, second press stops |
| `double` | Two rapid presses start a toggle session |

### Key Names

Keys use evdev names (Linux) or Virtual Key codes (Windows):

Common examples:
- `KEY_LEFTMETA` — Left Super/Windows key
- `KEY_LEFTCTRL` — Left Ctrl
- `KEY_LEFTSHIFT` — Left Shift  
- `KEY_SPACE` — Space bar
- `KEY_F12` — Function key 12

### Example bindings.toml

```toml
[[binding]]
id = "dictate_hold"
label = "Dictate to Cursor (Hold)"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_ids = ["default"]

[[binding]]
id = "dictate_notes"
label = "Dictate to Notes File"
keys = ["KEY_LEFTCTRL", "KEY_LEFTSHIFT", "KEY_N"]
gesture = "toggle"
target_ids = ["notes", "clipboard"]   # Routes to both sequentially

[[binding]]
id = "quick_copy"
label = "Copy to Clipboard"
keys = ["KEY_LEFTMETA", "KEY_V"]
gesture = "double"
target_ids = ["clipboard"]
disabled = false
```

### Multi-Target Routing

When `target_ids` contains multiple entries, VoxCtr delivers to each target **sequentially** in the listed order after a single recording session. This lets you, for example, inject text into a window AND log it to a file simultaneously.

---

## Router Logic

`OutputTargetRouter::route(text, target_id, targets)`:

1. Look up target by `target_id`
2. Apply any per-target `processing` overrides
3. Build delivery payload (append newline, prefix, timestamp)
4. Dispatch to the appropriate delivery handler
5. Log to history

On error (e.g. socket not available, file unwritable), VoxCtr logs the error and continues — it does not interrupt the UI or crash.
