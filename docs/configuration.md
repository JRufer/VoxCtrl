# Configuration Reference

VoxCtrl uses three configuration files, all stored under `~/.config/voxctrl/`.

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
    "backend": "whisper-cpp",
    "whisper_cpp": {
      "model_dir": "",
      "model_size": "tiny",
      "device": "auto",
      "threads": 0
    },
    "moonshine": {
      "model_size": "base",
      "language": "en"
    },
    "parakeet": {
      "model_size": "tdt-0.6b-v3",
      "language": "auto"
    },
    "remote_openai": {
      "endpoint": "http://localhost:8000/v1",
      "api_key": null,
      "model": "whisper-1",
      "language": "",
      "timeout_secs": 30
    },
    "s1_mini": {
      "enabled": false,
      "styling": "semi-formal"
    }
  },
  "audio": {
    "vad_threshold": 0.5,
    "input_device_index": null,
    "evdev_device": null,
    "noise_suppression": false,
    "gain": 1.0,
    "dynamic_stream": true
  },
  "ui": {
    "show_overlay": true,
    "show_command_overlay": true,
    "command_overlay_duration_secs": 3,
    "overlay_style": "blue_wave",
    "overlay_position": "center",
    "overlay_monitor": "primary",
    "auto_show_settings": false,
    "show_notification": false,
    "setup_completed": true
  },
  "features": {
    "remove_fillers": true,
    "custom_vocabulary": [],
    "spoken_punctuation": true,
    "auto_format_lists": true,
    "snippets": {}
  },
  "openai": {
    "enabled": false,
    "endpoint": "http://localhost:11434",
    "api_key": null,
    "model": "llama3.2:1b",
    "mode": "clean",
    "system_prompt": "Fix grammar and punctuation only. Return only the corrected text, no commentary.",
    "user_prompt": "{text}",
    "timeout_secs": 8
  },
  "tts": {
    "enabled": false,
    "engine": "piper",
    "voice": "en-us-lessac-medium",
    "voice_dir": "",
    "stop_key": ["KEY_ESCAPE"],
    "response_overlay": true,
    "speed": 1.0,
    "gpu": false,
    "hf_token": null,
    "pocket_tts": {
      "voice": "alba",
      "prewarm": false,
      "voice_dir": ""
    },
    "inflect_micro": {
      "model_dir": "",
      "seed": 0,
      "noise_scale": 0.667,
      "prewarm": false
    },
    "breeze_tts_2": {
      "voice_mode": "prompt",
      "speaker_prompt": "A calm and clear female voice speaking at a natural pace",
      "cloned_voice": "alba",
      "voice_dir": "",
      "model_dir": "",
      "prewarm": false,
      "gpu": false
    },
    "vox_cpm_2": {
      "voice_mode": "prompt",
      "speaker_prompt": "A calm young female voice speaking clearly with a gentle tone.",
      "cloned_voice": "alba",
      "voice_dir": "",
      "ultimate_cloning": false,
      "model_dir": "",
      "prewarm": false,
      "gpu": false
    },
    "memory_mode": "always_loaded",
    "idle_unload_secs": 900,
    "snippets": {
      "VoxCtrl": "Vox Control"
    }
  },
  "mcp": {
    "server_enabled": false,
    "record_timeout": 15.0,
    "visual_feedback": true
  },
  "updates": {
    "auto_check": true,
    "skipped_version": null
  }
}
```

### `engine` section


The engine config supports four distinct speech-recognition backends.

**Top-level fields:**

| Key | Type | Values | Description |
|---|---|---|---|
| `backend` | string | `"whisper-cpp"` (default), `"moonshine"`, `"parakeet"`, `"remote-openai"` | Which speech-recognition backend to use. |

**`whisper_cpp` sub-object:**

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"tiny"` | Whisper model to load (see valid values below). `tiny`/`tiny.en` auto-download silently on first launch; other sizes require an explicit download in Settings → Engine. |
| `device` | string | `"auto"` | Compute device: `auto`/`cpu`/`cuda`/`vulkan` |
| `threads` | integer | `0` | CPU thread count; 0 = one per physical core |
| `model_dir` | string | `""` | Custom model directory; empty = `~/.local/share/voxctrl/models/`. Supports `~` expansion. The directory must already exist. |

Valid `model_size` values: `tiny`, `tiny.en`, `base`, `base.en`, `small`, `small.en`, `medium`, `medium.en`, `large-v2`, `large-v3`, `large-v3-turbo`

The `.en` variants are English-only but slightly faster. `large-v3-turbo` is a distilled model balancing quality and speed.

**`moonshine` sub-object** (only used when `backend = "moonshine"`):

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"base"` | `"base"` or `"tiny"` |
| `language` | string | `"en"` | BCP-47 language code (output label only) |

> **Build requirement:** the Moonshine backend is a **default** compile-time
> feature, so a standard build includes it. It links ONNX Runtime, which is
> fetched at build time — a build made with `--no-default-features` omits it,
> and selecting `"moonshine"` then transparently falls back to `whisper-cpp`,
> still using the Whisper model configured above. The Settings → Engine panel
> shows whether Moonshine is available in the running build.

**`parakeet` sub-object** (only used when `backend = "parakeet"`):

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"tdt-0.6b-v3"` | Parakeet model size (INT8 ONNX) |
| `language` | string | `"auto"` | Target language code |

**`remote_openai` sub-object** (used when `backend = "remote-openai"` — Bring Your Own Voice Engine):

| Key | Type | Default | Description |
|---|---|---|---|
| `endpoint` | string | `"http://localhost:8000/v1"` | URL of the OpenAI-compatible speech-to-text service (e.g. `http://192.168.1.50:8000/v1` or `https://api.openai.com/v1`). |
| `api_key` | string or null | `null` | Optional Bearer token authentication. |
| `model` | string | `"whisper-1"` | Remote model name (e.g. `whisper-1`, `large-v3`). |
| `language` | string | `""` | Language code for transcription, or blank/auto. |
| `timeout_secs` | integer | `30` | Request timeout in seconds (min 5, max 300). |

**`s1_mini` sub-object** (dictation cleanup processor):

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable on-device text normalization using Superwhisper's fine-tuned S1-mini (Qwen3-0.6B) model. |
| `styling` | string | `"semi-formal"` | Text styling passed to S1-mini (`"semi-formal"`, `"formal"`, `"casual"`, etc.). |

> **Model files:** Enabling S1-mini requires downloading `s1-mini-q4_k_m.gguf` (~462 MB) and `tokenizer.json` (~11.4 MB) from Hugging Face (~480 MB download total). When toggled on in Settings → Engine, VoxCtrl automatically checks for these files in `~/.local/share/voxctrl/models/s1-mini/` and downloads them if missing.

### `audio` section

| Key | Type | Default | Description |
|---|---|---|---|
| `vad_threshold` | float | `0.5` | Voice Activity Detection sensitivity (0.0–1.0); **higher = more sensitive** (lower RMS gate) |
| `input_device_index` | integer or null | `null` | CPAL device index; null = auto-detect |
| `evdev_device` | string or null | `null` | Linux evdev keyboard device path for hotkeys, e.g. `"/dev/input/event4"` |
| `noise_suppression` | bool | `false` | Run captured audio through RNNoise before transcription. Needs a build with the `noisereduce` feature (the shipped app has it); see [audio.md](audio.md#noise-suppression) |
| `gain` | float | `1.0` | Microphone amplification multiplier |
| `dynamic_stream` | bool | `true` | Open mic on-demand (true) vs. always-on (false) |

**VAD threshold note:** The threshold maps as `rms_gate = (1.0 - vad_threshold) * 0.006`. At 0.5 (default), the gate threshold is 0.003 RMS. At 1.0 (maximum sensitivity), there is no gate (0.0 RMS). At 0.0 (minimum sensitivity), the gate is 0.006 RMS.

### `ui` section

| Key | Type | Allowed | Default | Description |
|---|---|---|---|---|
| `show_overlay` | bool | | `true` | Show visual HUD overlay during recording |
| `show_command_overlay` | bool | | `true` | Show temporary UI overlay pill when a voice command trigger is activated |
| `command_overlay_duration_secs` | integer | `1`–`10` | `3` | Duration in seconds to display the voice command overlay pill |
| `overlay_style` | string | `"voice_card"`, `"waveform"`, `"pulse"`, `"blue_wave"`, `"mono_bars"`, `"spectrum"`, `"terminal"`, `"vinyl"`, `"none"` | `"blue_wave"` | HUD visualization style |
| `overlay_position` | string | `"top"`, `"center"`, `"bottom"` | `"center"` | Screen positioning of the overlay window |
| `overlay_monitor` | string | `"primary"` or monitor name | `"primary"` | Specific display screen for visual overlay |
| `auto_show_settings` | bool | | `false` | Auto-show Settings window on startup |
| `show_notification` | bool | | `false` | Desktop toast notification after text delivery |
| `setup_completed` | bool | | `false` on a new install | Whether the first-run wizard has been finished. Absent from a config file written by an earlier VoxCtrl, which is read as `true` — an existing install has plainly been set up already, and must not be handed a setup wizard on upgrade |

### `features` section

| Key | Type | Default | Description |
|---|---|---|---|
| `remove_fillers` | bool | `true` | Strip filler words (`uh`, `um`, `hmm`, `er`, `ah`, etc.) |
| `spoken_punctuation` | bool | `true` | Convert spoken punctuation words to symbols (e.g. "period" → ".") |
| `auto_format_lists` | bool | `true` | Detect "first/second/third" patterns and reformat as a numbered list |
| `custom_vocabulary` | string[] | `[]` | Custom words; VoxCtrl uses fuzzy Levenshtein matching to correct near-matches post-transcription |
| `snippets` | object | `{}` | Short code → expansion map |

Example with snippets:
```json
"features": {
  "remove_fillers": true,
  "spoken_punctuation": true,
  "snippets": {
    "addr": "742 Evergreen Terrace, Springfield",
    "sig": "Best regards,\nAlice"
  }
}
```

### `openai` section

LLM post-processing through any OpenAI-compatible API server — a local server or
a hosted provider. Exposed in the GUI under **Settings → OpenAI API**.

> Configs written before this section was renamed used the key `ollama`; that key
> is still accepted (via a serde alias) and loads transparently into `openai`.

Each request sends two chat messages: the **system prompt** (how to transform the
text) and the **user prompt** (the message itself). The user prompt must contain
`{text}`, which is replaced with the dictated speech.

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable LLM post-processing |
| `endpoint` | string | `"http://localhost:11434"` | OpenAI-compatible API base URL. A `/v1` suffix is optional — it is added automatically when missing (e.g. requests go to `{endpoint}/v1/chat/completions`). |
| `api_key` | string or null | `null` | API key sent as a `Bearer` token. Required by most remote providers; usually unnecessary for a local server. |
| `model` | string | `"llama3.2:1b"` | Model name |
| `mode` | string | `"clean"` | Preset that fills the system prompt in the GUI: `clean`/`formal`/`casual`/`bullet`/`concise`/`custom`. Built-in presets are read-only in the GUI; choose `custom` to edit `system_prompt`/`user_prompt`. Generation itself is driven by `system_prompt`/`user_prompt`. |
| `system_prompt` | string | `"Fix grammar and punctuation only…"` | System message describing the transformation. Empty = no system message. |
| `user_prompt` | string | `"{text}"` | User message template. Must contain `{text}`, replaced with the dictated speech. |
| `timeout_secs` | integer | `8` | HTTP request timeout in seconds |

> The legacy `custom_prompt` field (used when `mode` was `custom`) is migrated
> into `user_prompt` automatically the first time an old config is loaded.

### `tts` section

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable TTS subsystem |
| `engine` | string | `"espeak"` | Synthesis engine: `"breeze_tts_2"`, `"vox_cpm_2"`, `"piper"`, `"pocket_tts"`, `"inflect_micro"`, or `"espeak"` |
| `voice` | string | `"en-us-lessac-medium"` | Active Piper voice name (hyphen-delimited, e.g. `"en-us-ryan-high"`) |
| `voice_dir` | string | `""` | Directory for Piper voice files; empty = `~/.local/share/voxctrl/piper-voices/`. Supports `~` expansion. |
| `stop_key` | string[] | `["KEY_ESCAPE"]` | Keys that cancel current TTS playback |
| `response_overlay` | bool | `true` | Show overlay indicator while TTS is speaking |
| `speed` | float | `1.0` | Speech synthesis speed multiplier (0.5 – 2.0); not used by Pocket-TTS |
| `gpu` | bool | `false` | Enable GPU acceleration (CUDA) for Piper |
| `breeze_tts_2` | object | | Breeze-TTS-2 engine sub-configuration (see below) |
| `vox_cpm_2` | object | | VoxCPM2 engine sub-configuration (see below) |
| `pocket_tts` | object | | Pocket-TTS engine sub-configuration (see below) |
| `inflect_micro` | object | | Inflect-Micro-v2 engine sub-configuration (see below) |
| `memory_mode` | string | `"always_loaded"` | `"always_loaded"` keeps the model resident for the session; `"on_demand"` loads it when it is needed and unloads it again after `idle_unload_secs` of no use. Affects the model-backed engines (Pocket-TTS, Breeze-TTS-2, VoxCPM2, Inflect-Micro-v2) — Piper and eSpeak hold no model between utterances. |
| `idle_unload_secs` | int | `900` (15 min) | Idle time before the model is unloaded in `"on_demand"` mode. The countdown restarts on every use. Values below 30s are clamped to 30s. |
| `snippets` | object | | Word/phrase pronunciation overrides expanded prior to synthesis (e.g. `{"VoxCtrl": "Vox Control"}`) |

**`breeze_tts_2` sub-object:**

[Breeze-TTS-2](https://huggingface.co/BreezeBlue/Breeze-TTS-2) is a bilingual speech generation model with natural-language voice design speaker prompts and reference voice cloning. The model weights are gated on HuggingFace under the **BreezeBlue Research and Non-Commercial License** — supply your access token via `tts.hf_token`, the single token shared by every gated model, or export it as `HF_TOKEN`, which takes precedence and is never written to the config.

| Key | Type | Default | Description |
|---|---|---|---|
| `voice_mode` | string | `"prompt"` | `"prompt"` for Voice Design, `"clone"` to use a reference audio clip from `voice_dir` |
| `speaker_prompt` | string | `"A calm and clear female voice speaking at a natural pace"` | Natural-language prompt describing the desired speaker voice for Voice Design |
| `cloned_voice` | string | `"alba"` | Voice ID from the shared voice folder, used in `"clone"` mode |
| `voice_dir` | string | `""` | Directory holding reference clips; empty = `~/.local/share/voxctrl/pocket-tts-voices/` |
| `model_dir` | string | `""` | Directory holding model weights & tokenizer; empty = `~/.local/share/voxctrl/models/breeze-tts-2/` |
| `prewarm` | bool | `false` | Pre-warm model weights and tensors on startup so first speech is instantaneous |
| `gpu` | bool | `false` | Run synthesis on the GPU. Needs a build with the `breeze-cuda` or `breeze-metal` feature; falls back to the CPU otherwise |

**`vox_cpm_2` sub-object:**

[VoxCPM2](https://huggingface.co/openbmb/VoxCPM2) is an Apache-2.0 open-weight model by OpenBMB offering high quality 24 kHz speech synthesis with Voice Design and Voice Cloning.

| Key | Type | Default | Description |
|---|---|---|---|
| `voice_mode` | string | `"prompt"` | `"prompt"` for Voice Design, `"clone"` for reference voice clip cloning |
| `speaker_prompt` | string | `"A calm young female voice speaking clearly with a gentle tone."` | Natural-language prompt for Voice Design |
| `cloned_voice` | string | `"alba"` | Reference voice clip ID from `voice_dir` |
| `voice_dir` | string | `""` | Directory holding custom `.wav` reference clips; empty = `~/.local/share/voxctrl/pocket-tts-voices/` |
| `ultimate_cloning` | bool | `false` | Enables Ultimate Cloning when paired audio + transcript files are available |
| `model_dir` | string | `""` | Directory holding model weights; empty = `~/.local/share/voxctrl/models/voxcpm2/` |
| `prewarm` | bool | `false` | Pre-warm model on startup for instantaneous first synthesis |
| `gpu` | bool | `false` | Enable CUDA GPU acceleration |

**`pocket_tts` sub-object:**

Pocket-TTS is a voice-cloning neural TTS engine: each voice is a short reference audio clip
that conditions synthesis, rather than a fixed precomputed voice embedding. The model weights
are hosted in a **gated** HuggingFace repository (`kyutai/pocket-tts`) — you must accept the
license on HuggingFace and supply a personal access token via `tts.hf_token`.

| Key | Type | Default | Description |
|---|---|---|---|
| `voice` | string | `"alba"` | Bundled reference voice ID (`"alba"`, `"anna"`, `"vera"`, `"charles"`, `"michael"`), or the filename stem of a custom clip in `voice_dir` |
| `prewarm` | bool | `false` | Pre-warm model on startup so first speech is instantaneous |
| `voice_dir` | string | `""` | Directory scanned for custom `.wav` voice clips; empty = `~/.local/share/voxctrl/pocket-tts-voices/`. Drop a `<id>.wav` file in to add it to the voice list — naming it after a built-in voice (e.g. `alba.wav`) overrides that voice's clip. Supports `~` expansion. |

**`inflect_micro` sub-object:**

[Inflect-Micro-v2](https://huggingface.co/owensong/Inflect-Micro-v2) is a ~9.4M-parameter VITS-family
ONNX model with a single fixed English voice at 24 kHz, so it has no voice setting. Requires the
`inflect-micro` cargo feature at build time and `espeak-ng` at runtime; speaking rate comes from the
shared `tts.speed`. See [tts.md](tts.md) for the full engine notes.

| Key | Type | Default | Description |
|---|---|---|---|
| `model_dir` | string | `""` | Directory holding the ONNX graphs and phoneme vocabulary; empty = `~/.local/share/voxctrl/models/inflect-micro/`. Point at an existing copy to skip downloading. Supports `~` expansion. |
| `seed` | int | `0` | Sampling seed. The model is deterministic for a fixed seed, so repeated synthesis of the same text is identical. |
| `noise_scale` | float | `0.667` | Latent sampling temperature (0.0 – 1.0) — higher is more varied, lower is flatter |
| `prewarm` | bool | `false` | Load the ONNX graphs on startup so the first synthesis has no load delay |


### `mcp` section

| Key | Type | Default | Description |
|---|---|---|---|
| `server_enabled` | bool | `false` | Start the MCP socket server on launch |
| `record_timeout` | float | `15.0` | How long `transcribe_voice` listens when the MCP client passes no `timeout_seconds`. Read per call, so a change applies without restarting the server; a non-positive value falls back to `15.0` |
| `visual_feedback` | bool | `true` | Show overlay indicator while MCP server is listening to microphone |

### `updates` section

| Key | Type | Default | Description |
|---|---|---|---|
| `auto_check` | bool | `true` | Ask GitHub for the latest release ~10s after launch, and offer it if it is newer |
| `skipped_version` | string \| null | `null` | A version the user pressed "Skip this version" on; a newer release is still offered |

This is the only part of VoxCtrl that reaches the network without being asked
to. The request is an unauthenticated `GET` to
`api.github.com/repos/JRufer/VoxCtrl/releases/latest`, sending a `User-Agent` of
`VoxCtrl/<version>` and nothing else — no machine, user or install identifier.
Set `auto_check` to `false` (or untick Settings → General → "Check for a new
version on launch") and VoxCtrl never contacts GitHub unless you press "Check
now". See [privacy.md](privacy.md#network).

Missing from an older `config.json`, both keys take the defaults above.

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

### All common fields
```toml
[[target]]
id = "my_target"
label = "My Target"
delivery = "inject"

# Text formatting
strip_newlines = false        # Default: false. Replaces newlines with spaces and strips \r (inject and command targets)

# Per-target post-processing overrides (all optional; null = inherit global)
[my_target.processing]
remove_fillers = true
spoken_punctuation = true
auto_format_lists = true
code_mode = false
```

### Delivery-specific fields

**`file`:**
```toml
delivery = "file"
file_path = "~/Documents/notes.md"
file_prefix = "- "
file_timestamp = true          # Default: true
file_timestamp_format = "%Y-%m-%dT%H:%M:%SZ"   # strftime pattern, rendered in UTC
file_mode = "append"           # "append" or "write"
```

**`http`:**
```toml
delivery = "http"
http_url = "http://localhost:8080/voice"
http_method = "POST"           # Default: "POST"
# Optional: custom headers and JSON template
```

**`webhook`:**
```toml
delivery = "webhook"
webhook_url = "https://example.com/hook"
webhook_secret = "my-hmac-secret"
```

Note: `webhook` uses `webhook_url`, while `http` uses `http_url`.

**`exec`:**
```toml
delivery = "exec"
command = "/usr/local/bin/handle-voice.sh"
```

**`socket`** (supports Unix and TCP):
```toml
delivery = "socket"
socket_unix = "/tmp/myapp.sock"    # Unix domain socket
# OR:
socket_host = "127.0.0.1"
socket_port = 9000
```

**`pipe`:**
```toml
delivery = "pipe"
pipe_path = "/tmp/voice.fifo"
```

**`speak`:**
```toml
delivery = "speak"
```

**`command`** — Voice Command Router (routes speech based on spoken target names):
```toml
delivery = "command"
```
See [Output Routing](routing.md#command--voice-command-router) for dynamic keyword parsing rules and conversational lead-in examples.

**`chat`** — conversational LLM over an OpenAI-compatible API, with history:
```toml
delivery = "chat"
chat_url = "http://localhost:8080"     # /v1 suffix optional
chat_model = "hermes-4-14b"
chat_system_prompt = "You are a concise voice assistant."
chat_reply_mode = "speak"              # speak | inject | clipboard | none
chat_max_history = 20                  # messages sent per turn; 0 = whole conversation
chat_timeout_secs = 120
chat_reset_phrase = "new conversation" # optional spoken phrase that clears history
# chat_api_key = "sk-..."              # optional bearer token
```
See [Output Routing](routing.md#chat--conversational-llm-openai-compatible) for details.

**TTS response pipe:**
```toml
response_pipe = "/tmp/tts-response.fifo"  # Optional FIFO for TTS output
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
hold_threshold_ms = 200        # Default: 200ms min hold to register
disabled = false
openai_enabled = true          # Enable LLM rewrite specifically for this hotkey
openai_mode = "formal"         # Rewrite output in formal style for this hotkey

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
gesture = "double_tap"
tap_ms = 250                   # Default: 250ms inter-tap window
target_ids = ["code_editor"]

[[binding]]
id = "double_tap_hold_dictate"
label = "Double-Tap & Hold to Dictate"
keys = ["KEY_LEFTMETA", "KEY_SPACE"]
gesture = "double_tap_hold"
tap_ms = 300
hold_threshold_ms = 200        # Hold threshold on the second tap
target_ids = ["default"]

[[binding]]
id = "toggle_dictation"
label = "Toggle Dictation"
keys = ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"]
gesture = "toggle"
target_ids = ["default"]
```

### Field reference

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `id` | string | Yes | | Unique identifier |
| `label` | string | Yes | `""` | The target's name; also the phrase that routes dictation here through a `command` target |
| `keys` | string[] | Yes | | Key names (evdev format, on every platform). Any number of modifiers plus exactly one regular key — see [Hotkeys](hotkeys.md#what-can-be-a-shortcut) |
| `gesture` | string | Yes | | `"hold"`, `"toggle"`, `"double_tap"`, or `"double_tap_hold"` |
| `target_ids` | string[] | Yes | | Ordered list of target IDs to route to |
| `target_id` | string | No | | Single target (legacy; use `target_ids`) |
| `hold_threshold_ms` | integer | No | `200` | Min hold duration in ms for hold / double-tap-hold gesture |
| `tap_ms` | integer | No | `300` | Longest gap between the first tap's release and the second tap's press, in ms |
| `disabled` | bool | No | `false` | Disable without deleting |
| `openai_enabled` | bool | No | `null` | Enable/disable LLM post-processing specifically for this hotkey (null = inherit global config) |
| `openai_model` | string | No | `null` | LLM model override specifically for this hotkey |
| `openai_mode` | string | No | `null` | LLM mode override specifically for this hotkey (`clean`/`formal`/`casual`/`bullet`/`concise`/`custom`) |
| `openai_prompt` | string | No | `null` | User prompt template override for this hotkey (must contain `{text}`) |
| `openai_system_prompt` | string | No | `null` | System prompt override for this hotkey (empty inherits the global default) |
| `s1_mini_enabled` | bool | No | `null` | Enable/disable S1-mini dictation cleanup specifically for this hotkey (null = inherit global engine config) |

> The per-hotkey field names were renamed from `ollama_*` to `openai_*`; the
> legacy `ollama_*` names are still accepted via serde aliases.

---

## Config Migration

VoxCtrl auto-migrates older config formats on load. Known migrations:

- `features.show_notification` → `ui.show_notification` (moved in an early release; the migrated config is immediately re-saved to disk to clean up the old key)

Unrecognized fields are silently ignored. Missing fields use their defaults. This ensures compatibility when upgrading or downgrading.
