# Text-to-Speech

**Crate:** `crates/voxctr-tts/`

## Overview

VoxCtr includes a neural TTS engine for voice output. This is useful for reading back transcriptions, confirming commands, or building conversational voice interactions via the MCP server.

---

## Engines

### Piper (Primary)
[Piper](https://github.com/rhasspy/piper) is a fast, local neural TTS system using ONNX models. It produces high-quality natural-sounding speech entirely offline.

VoxCtr invokes the `piper` binary directly (looks first in `~/.local/share/voxctl/piper/piper`, then on PATH). It pipes text to Piper's stdin, receives raw 16-bit PCM on stdout, and plays via `aplay` (Linux) or a temp WAV file + PowerShell `SoundPlayer` (Windows).

### Espeak-ng (Fallback)
If Piper is unavailable or no voice is downloaded, VoxCtr falls back to `espeak-ng`. It is invoked as a subprocess with the text as an argument. Quality is lower but espeak-ng is always available as a system package.

---

## Voice Catalogue

Voices are downloaded as `.tar.gz` archives from the Piper GitHub release (`v0.0.2`). Extracted `.onnx` and `.onnx.json` files are stored at `~/.local/share/voxctl/piper-voices/`.

| Voice name | Quality | Sample rate |
|---|---|---|
| `en-us-libritts-high` | high | 22050 Hz |
| `en-us-ryan-high` | high | 22050 Hz |
| `en-us-ryan-medium` | medium | 22050 Hz |
| `en-us-ryan-low` | low | 16000 Hz |
| `en-us-lessac-medium` | medium | 16000 Hz |
| `en-us-lessac-low` | low | 16000 Hz |
| `en-us-amy-low` | low | 16000 Hz |
| `en-us-kathleen-low` | low | 16000 Hz |
| `en-us-danny-low` | low | 16000 Hz |
| `en-gb-southern_english_female-low` | low | 16000 Hz |
| `en-gb-alan-low` | low | 16000 Hz |

The default voice is **`en-us-lessac-medium`**.

---

## Voice Packs

### Checking if a voice is downloaded
```typescript
const downloaded = await invoke<boolean>('check_voice_downloaded', {
  voiceName: 'en-us-lessac-medium'
});
```

### Downloading a voice
Via the UI: Settings → TTS → select a voice → click Download.

Via IPC:
```typescript
await invoke('download_voice', { voiceName: 'en-us-ryan-high' });
```

The download fetches `voice-<name>.tar.gz` from the Piper GitHub release, extracts the `.onnx` and `.onnx.json` files into the voices directory, and uses atomic temp-file writes to avoid partial downloads.

---

## Audio Playback

After synthesis, raw PCM audio is played using the system audio stack:

- **Linux:** `aplay -r <sample_rate> -f S16_LE -c 1 -` (ALSA, streaming from stdin)
- **Windows:** Writes a temp WAV file, plays with PowerShell `[System.Media.SoundPlayer]::PlaySync()`

The TTS engine queues requests in a bounded channel (capacity 32). Utterances play sequentially — subsequent calls are queued and played in order without overlapping.

---

## Triggering TTS

### From the MCP Server
```json
{"method": "tools/call", "params": {"name": "speak_text", "arguments": {"text": "Recording complete."}}}
```

### From a Tauri IPC Command
```typescript
await invoke('speak_text', { text: 'Hello world', voice: 'en-us-ryan-high' });
```

The `voice` parameter is optional; if omitted, the configured default voice is used.

### From a FIFO Response Pipe
If a target has a `response_pipe` path configured, VoxCtr watches that FIFO for newline-terminated text and speaks each line:

```bash
echo "Recording started" > /tmp/voxctl-tts.fifo
```

Multiple targets can have different `response_pipe` paths. VoxCtr spawns a FIFO watcher task for each unique path when targets are loaded or updated.

---

## Stopping Playback

The `stop_key` (singular, an array) config field lists keys that interrupt current TTS playback when pressed:

```json
"tts": {
  "stop_key": ["KEY_ESCAPE"]
}
```

Sending `None` through the TTS engine channel (via `TtsEngineHandle::stop()`) clears the current utterance.

---

## Configuration Options

Under `tts` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable TTS functionality |
| `engine` | string | `"piper"` | `"piper"` or `"espeak"` |
| `voice` | string | `"en-us-lessac-medium"` | Voice name (hyphen-delimited) |
| `stop_key` | string[] | `["KEY_ESCAPE"]` | Keys that interrupt playback |
| `response_overlay` | bool | `true` | Show overlay indicator while TTS is speaking |

---

## TtsEngineHandle

The TTS handle is stored in `AppState` and shared with the MCP server, routing system, and IPC commands:

```rust
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,      // None = use config default
    pub source_label: Option<String>,
}

pub struct TtsEngineHandle {
    tx: Sender<Option<Utterance>>,  // None = stop signal
}

impl TtsEngineHandle {
    pub fn speak(&self, text: impl Into<String>);
    pub fn speak_utterance(&self, u: Utterance);
    pub fn stop(&self);  // sends None, clears current playback
}
```

The handle is `Clone` — multiple callers can hold a copy and enqueue utterances concurrently.
