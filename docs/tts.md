# Text-to-Speech

**Crate:** `crates/voxctr-tts/`

## Overview

VoxCtr includes a neural TTS engine for voice output. This is useful for reading back transcriptions, confirming commands, or building conversational voice interactions via the MCP server.

---

## Engines

### Piper (Primary)
[Piper](https://github.com/rhasspy/piper) is a fast, local neural TTS system using ONNX models. It produces high-quality natural-sounding speech entirely offline.

VoxCtr ships with support for approximately 11 English voices from the Piper v0.0.2 release.

Voice files are stored at `~/.local/share/voxctl/piper-voices/`. Each voice requires:
- `voice-name.onnx` — the neural network model
- `voice-name.onnx.json` — model configuration

### Espeak-ng (Fallback)
If Piper is unavailable or no voice is downloaded, VoxCtr falls back to [espeak-ng](https://github.com/espeak-ng/espeak-ng), a formant-synthesis engine that requires no downloads. Quality is lower but it is always available.

---

## Voice Packs

### Available Voices

Voices are downloaded from the Piper GitHub release. Available English voices include (names are illustrative):

- `en_US-lessac-medium`
- `en_US-libritts-high`
- `en_US-ryan-high`
- `en_US-kusal-medium`
- `en_US-joe-medium`
- `en_GB-alan-medium`
- `en_GB-jenny_dioco-medium`

Check the Settings → TTS tab for the full current list and download status.

### Downloading Voices

Via the UI: Settings → TTS → select a voice → click Download.

Via IPC:
```typescript
await invoke('download_voice', { name: 'en_US-lessac-medium' });
```

Via the Tauri command from Rust:
```rust
download_voice("en_US-lessac-medium").await?;
```

---

## Audio Playback

After synthesis, VoxCtr plays audio using the system audio stack:

- **Linux:** `aplay` (ALSA)
- **Windows:** PowerShell `[System.Media.SoundPlayer]`

The audio plays asynchronously — the TTS engine queues requests and a dedicated task drains the queue sequentially so speech doesn't overlap.

---

## Triggering TTS

### From the MCP Server
LLM agents can queue speech via the `speak_text` MCP tool:
```json
{"method": "tools/call", "params": {"name": "speak_text", "arguments": {"text": "Recording complete."}}}
```

### From a Hotkey Binding Target
Route to a TTS target — the `tts_engine` field on a target specifies which voice to use.

### From the IPC Command
```typescript
await invoke('speak_text', { text: 'Hello world', voice: 'en_US-lessac-medium' });
```

### From the FIFO Pipe
VoxCtr watches a named FIFO for newline-terminated text to speak. Write to it from any script:
```bash
echo "Recording started" > /tmp/voxctl-tts.fifo
```

---

## Stop Keys

TTS playback can be interrupted with a configurable key combination. Set `tts.stop_keys` in config:

```json
"tts": {
  "stop_keys": ["KEY_ESC"]
}
```

When the stop key is pressed during playback, the current utterance is cancelled and the queue is cleared.

---

## Configuration Options

Under `tts` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable TTS functionality |
| `engine` | string | `"piper"` | `"piper"` or `"espeak"` |
| `voice` | string | `"en_US-lessac-medium"` | Voice name for Piper |
| `stop_keys` | string[] | `["KEY_ESC"]` | Keys that interrupt playback |
| `rate` | int | `175` | Speech rate in words-per-minute (espeak only) |
| `pitch` | int | `50` | Pitch level 0-99 (espeak only) |

---

## TtsEngineHandle

The TTS handle is stored in `AppState` and shared with the MCP server and routing system:

```rust
pub struct TtsEngineHandle {
    tx: mpsc::Sender<TtsRequest>,
}

pub struct TtsRequest {
    pub text: String,
    pub voice: Option<String>,
}
```

Send a request by cloning the handle and sending on the channel. The worker task picks it up and synthesizes/plays sequentially.
