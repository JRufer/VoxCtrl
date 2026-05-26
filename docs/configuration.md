# Configuration Reference

VoxCtr uses three configuration files, all stored under `~/.config/voxctl/`.

| File | Format | Purpose |
|---|---|---|
| `config.json` | JSON | Application settings |
| `targets.toml` | TOML | Output target definitions |
| `bindings.toml` | TOML | Hotkey binding definitions |

All files are **hot-reloaded** — the app watches for external changes and applies them without restart.

---

## config.json

Full schema with defaults:

```json
{
  "engine": {
    "backend": "whisper",
    "model_size": "base",
    "device": "auto",
    "language": "en"
  },
  "audio": {
    "device_index": -1,
    "gain": 1.0,
    "vad_threshold": 0.02,
    "dynamic_stream": true
  },
  "ui": {
    "overlay_style": "ocean_wave",
    "auto_show_overlay": true,
    "show_notification": false,
    "history_enabled": true
  },
  "features": {
    "remove_fillers": true,
    "snippets": {},
    "vocabulary": []
  },
  "ollama": {
    "enabled": false,
    "endpoint": "http://localhost:11434",
    "model": "llama3.2",
    "mode": "clean",
    "custom_prompt": ""
  },
  "tts": {
    "enabled": false,
    "engine": "piper",
    "voice": "en_US-lessac-medium",
    "stop_keys": ["KEY_ESC"]
  },
  "mcp": {
    "enabled": false,
    "record_timeout": 30
  },
  "atspi": {
    "enabled": false,
    "context_prompt": false,
    "auto_code_mode": false
  }
}
```

### `engine` section

| Key | Type | Values | Description |
|---|---|---|---|
| `backend` | string | `"whisper"`, `"moonshine"` | Recognition engine |
| `model_size` | string | `"tiny"` `"base"` `"small"` `"medium"` `"large-v3"` | Whisper model to load |
| `device` | string | `"auto"` `"cpu"` `"cuda"` `"vulkan"` | Compute backend |
| `language` | string | ISO 639-1 code or `"auto"` | Transcription language |

### `audio` section

| Key | Type | Range | Description |
|---|---|---|---|
| `device_index` | integer | `-1` = auto | CPAL device index |
| `gain` | float | `0.0` – `4.0` | Microphone amplification |
| `vad_threshold` | float | `0.0` – `1.0` | RMS energy cutoff for silence detection |
| `dynamic_stream` | bool | | Open mic on-demand vs. always-on |

### `ui` section

| Key | Type | Values | Description |
|---|---|---|---|
| `overlay_style` | string | `"ocean_wave"` `"voice_card"` `"waveform"` `"pulse"` | HUD visualization |
| `auto_show_overlay` | bool | | Show overlay automatically when recording |
| `show_notification` | bool | | Desktop toast notification on text delivery |
| `history_enabled` | bool | | Enable transcription history tracking |

### `features` section

| Key | Type | Description |
|---|---|---|
| `remove_fillers` | bool | Strip "um", "uh", "hmm" from output |
| `snippets` | object | Short code → expansion map |
| `vocabulary` | string[] | Custom words to improve Whisper accuracy |

Example with snippets:
```json
"features": {
  "remove_fillers": true,
  "snippets": {
    "addr": "742 Evergreen Terrace, Springfield",
    "sig": "Best regards,\nAlice",
    "mtg": "meeting"
  }
}
```

### `ollama` section

| Key | Type | Values | Description |
|---|---|---|---|
| `enabled` | bool | | Enable Ollama post-processing |
| `endpoint` | string | URL | Ollama API base URL |
| `model` | string | | Model name (e.g. `"llama3.2"`, `"phi3"`) |
| `mode` | string | `"clean"` `"formal"` `"casual"` `"bullet"` `"concise"` `"custom"` | Rewrite style |
| `custom_prompt` | string | | Instruction when mode is `"custom"` |

### `tts` section

| Key | Type | Values | Description |
|---|---|---|---|
| `enabled` | bool | | Enable TTS subsystem |
| `engine` | string | `"piper"`, `"espeak"` | Synthesis engine |
| `voice` | string | Voice name | Active Piper voice |
| `stop_keys` | string[] | evdev key names | Keys that cancel playback |

### `mcp` section

| Key | Type | Description |
|---|---|---|
| `enabled` | bool | Start the MCP socket server |
| `record_timeout` | integer | Max seconds for `transcribe_voice` to wait |

### `atspi` section

| Key | Type | Description |
|---|---|---|
| `enabled` | bool | Enable AT-SPI2 integration |
| `context_prompt` | bool | Read focused widget text for Whisper context |
| `auto_code_mode` | bool | Detect code editors and enable code-mode automatically |

---

## targets.toml

Each output destination is a `[[target]]` block.

### Minimal example
```toml
[[target]]
id = "default"
label = "Focused Window"
delivery = "inject"
```

### Full example with all common fields
```toml
[[target]]
id = "my_target"
label = "My Target"
delivery = "inject"          # See delivery types below
append_newline = false        # Append \n after text
send_on_release = false       # Wait for hotkey release
initial_prompt = ""           # Whisper context prompt override

[my_target.processing]
code_mode = false
remove_fillers = true
ollama_enabled = false
```

### Delivery-specific fields

**`file`:**
```toml
delivery = "file"
file_path = "~/Documents/notes.md"
file_prefix = "- "
file_timestamp = true
```

**`http` / `webhook`:**
```toml
delivery = "webhook"
http_url = "https://example.com/hook"
webhook_secret = "my-hmac-secret"
```

**`exec`:**
```toml
delivery = "exec"
exec_command = "/usr/local/bin/handle-voice.sh"
```

**`socket`:**
```toml
delivery = "socket"
socket_path = "/tmp/myapp.sock"
```

**`pipe`:**
```toml
delivery = "pipe"
pipe_path = "/tmp/voice.fifo"
```

---

## bindings.toml

Each hotkey is a `[[binding]]` block.

### Full example
```toml
[[binding]]
id = "dictate_hold"
label = "Dictate (Hold)"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "hold"
target_ids = ["default"]
disabled = false

[[binding]]
id = "dictate_notes"
label = "Dictate to Notes + Clipboard"
keys = ["KEY_LEFTCTRL", "KEY_LEFTSHIFT", "KEY_N"]
gesture = "toggle"
target_ids = ["notes", "clipboard"]

[[binding]]
id = "quick_code"
label = "Code Dictation (Double-tap)"
keys = ["KEY_F12"]
gesture = "double"
target_ids = ["code_editor"]
disabled = false
```

### Field reference

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | string | Yes | Unique identifier |
| `label` | string | Yes | Display name |
| `keys` | string[] | Yes | Key names (evdev format) |
| `gesture` | string | Yes | `"hold"` / `"toggle"` / `"double"` |
| `target_ids` | string[] | Yes | Ordered list of target IDs |
| `disabled` | bool | No | Disable without deleting |

---

## Config Migration

VoxCtr auto-migrates older config formats. Known migrations:

- `features.show_notification` → `ui.show_notification` (moved in an early release)

If a field is unrecognized, it is ignored. Missing fields use their defaults. This ensures forward and backward compatibility when upgrading.
