# Text-to-Speech

**Crate:** `crates/voxctrl-tts/`

## Overview

VoxCtrl includes a neural TTS engine for voice output. This is useful for reading back transcriptions, confirming commands, or building conversational voice interactions via the MCP server.

---

## Engines

### Piper (Primary)
[Piper](https://github.com/rhasspy/piper) is a fast, local neural TTS system using ONNX models. It produces high-quality natural-sounding speech entirely offline.

VoxCtrl invokes the `piper` binary directly (looks first in `~/.local/share/voxctrl/piper/piper`, then on PATH). It pipes text to Piper's stdin, receives raw 16-bit PCM on stdout, and plays via rodio (cross-platform).

### Kokoro (Neural, Natural)
[Kokoro](https://github.com/hexgrad/kokoro) is a high-quality, natural-sounding neural TTS engine with English voices (American and British accents). VoxCtrl runs Kokoro entirely in Rust: text is phonemised via `espeak-ng`, tokenised against an embedded IPA vocabulary, and synthesised via native ONNX inference (`ort` crate). No Python is required.

**Prerequisites:**
- `espeak-ng` installed on the system (`apt install espeak-ng` on Debian/Ubuntu)

Model files are downloaded from GitHub releases to `~/.local/share/voxctrl/kokoro/`. During download, the voices pack is automatically unzipped into the `voices/` subdirectory, and the zip archive is deleted to save space and avoid ZIP-parsing disk overhead at runtime. Audio is played via rodio using the raw PCM output from the ONNX model.

**Model quality levels:**

| Quality | File | Size | Use case |
|---|---|---|---|
| `f32` | `kokoro-v1.0.onnx` | 310 MB | Highest quality (recommended with GPU acceleration) |
| `fp16` | `kokoro-v1.0.fp16.onnx` | 169 MB | Default (balanced quality, smaller) |

### Espeak-ng (Lightweight)
If Piper is unavailable or no voice is downloaded, VoxCtrl can use `espeak-ng`. It is invoked as a subprocess with the text as an argument. Quality is lower but espeak-ng is always available as a system package.

---

## GPU Acceleration

VoxCtrl supports **GPU Acceleration** for both the **Kokoro** and **Piper** neural engines:

*   **Kokoro:** Enables the ONNX Runtime CUDA Execution Provider natively inside the Rust backend.
*   **Piper:** Appends the `--cuda` CLI flag to the spawned `piper` subprocess command dynamically at runtime.

### Requirements & Setup:
1.  A CUDA-compatible NVIDIA GPU and drivers installed.
2.  Pointing VoxCtrl to a GPU-enabled ONNX Runtime shared library. Because the app loads the shared library dynamically via `load-dynamic`, you can link the GPU library (e.g. from Python's `onnxruntime-gpu`) using the `ORT_DYLIB_PATH` environment variable:
    ```bash
    export ORT_DYLIB_PATH=/path/to/libonnxruntime.so
    cargo tauri dev --features cuda
    ```
    If GPU initialization fails, the engines will automatically and gracefully fall back to executing on the **CPU** without causing a crash.

---

## Voice Catalogue

### Piper Voices

Voices are downloaded as `.tar.gz` archives from the Piper GitHub release (`v0.0.2`). Extracted `.onnx` and `.onnx.json` files are stored in the configured voice directory (see [Configuration Options](#configuration-options) below). The default is `~/.local/share/voxctrl/piper-voices/`.

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

### Kokoro Voices

Kokoro ships voices split across four accent groups. All voices share the same model and voices pack (`voices-v1.0.bin`). Switching voices requires no additional downloads.

**American Female (`af_*`)**
| ID | Name |
|---|---|
| `af_heart` | Heart (default) |
| `af_bella` | Bella |
| `af_sarah` | Sarah |
| `af_nicole` | Nicole |
| `af_sky` | Sky |
| `af_alloy` | Alloy |
| `af_aoede` | Aoede |
| `af_jessica` | Jessica |
| `af_kore` | Kore |
| `af_nova` | Nova |
| `af_river` | River |

**American Male (`am_*`)**
| ID | Name |
|---|---|
| `am_adam` | Adam |
| `am_michael` | Michael |
| `am_puck` | Puck |
| `am_echo` | Echo |
| `am_eric` | Eric |
| `am_fenrir` | Fenrir |
| `am_liam` | Liam |
| `am_onyx` | Onyx |
| `am_santa` | Santa |

**British Female (`bf_*`)**
| ID | Name |
|---|---|
| `bf_emma` | Emma |
| `bf_alice` | Alice |
| `bf_isabella` | Isabella |
| `bf_lily` | Lily |

**British Male (`bm_*`)**
| ID | Name |
|---|---|
| `bm_george` | George |
| `bm_lewis` | Lewis |
| `bm_daniel` | Daniel |
| `bm_fable` | Fable |

---

## Voice Packs

### Piper — Checking and downloading

```typescript
// Check
const downloaded = await invoke<boolean>('check_voice_downloaded', {
  voiceName: 'en-us-lessac-medium',
  voiceDir: '',           // '' = use default directory
});

// Download
await invoke('download_voice', {
  voiceName: 'en-us-ryan-high',
  voiceDir: '',           // '' = default; or a custom path, e.g. '~/my-voices'
});
```

### Kokoro — Checking and downloading

Kokoro downloads the model file and the voices pack ZIP file. It automatically extracts the ZIP into a `voices/` directory (containing standalone `{voice}.npy` files) and deletes the ZIP archive to save space.

```typescript
// Check if model files and unzipped voices folder are present
const ready = await invoke<boolean>('check_kokoro_ready', {
  quality: 'fp16',        // "f32" | "fp16"
  dataDir: '',            // '' = ~/.local/share/voxctrl/kokoro/
});

// Download and automatically extract model and voices pack
await invoke('download_kokoro', {
  quality: 'fp16',
  dataDir: '',
});
```

---

## Audio Playback

After synthesis, audio is played using `rodio` (cross-platform):

- **Piper** produces raw 16-bit signed LE PCM; rodio plays it directly via `SamplesBuffer`.
- **Kokoro** produces raw float32 PCM samples from ONNX inference; samples are converted to i16 and played via `SamplesBuffer` at 24 kHz.

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
// For Kokoro the voice parameter overrides cfg.tts.kokoro.voice:
await invoke('speak_text', { text: 'Hello world', voice: 'af_bella' });
```

The `voice` parameter is optional; if omitted, the configured default voice is used.

### From a FIFO Response Pipe
If a target has a `response_pipe` path configured, VoxCtrl watches that FIFO for newline-terminated text and speaks each line:

```bash
echo "Recording started" > /tmp/voxctrl-tts.fifo
```

---

## Pre-warming Kokoro

Kokoro loads its ONNX model on first synthesis. Enable `prewarm` to avoid this latency:

```json
"tts": {
  "engine": "kokoro",
  "kokoro": { "prewarm": true }
}
```

When `prewarm` is `true`, `TtsEngineWorker::start()` enqueues a silent synthesis immediately after spawning the worker thread. The worker processes this short request (a single space) at startup, loading the model files into the OS page cache. Subsequent user-triggered syntheses are faster because the model is already in memory. This adds roughly 5–15 seconds to startup time depending on model size and disk speed.

---

## Stopping Playback

The `stop_key` config field lists keys that interrupt current TTS playback when pressed:

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
| `engine` | string | `"piper"` | `"piper"`, `"kokoro"`, or `"espeak"` |
| `voice` | string | `"en-us-lessac-medium"` | Default voice for Piper (hyphen-delimited) |
| `voice_dir` | string | `""` | Directory for Piper voice files; empty = `~/.local/share/voxctrl/piper-voices/` |
| `stop_key` | string[] | `["KEY_ESCAPE"]` | Keys that interrupt playback |
| `response_overlay` | bool | `true` | Show overlay indicator while TTS is speaking |
| `gpu` | bool | `false` | Enable GPU acceleration (CUDA) for Kokoro and Piper |
| `kokoro.voice` | string | `"af_heart"` | Default Kokoro voice ID |
| `kokoro.quality` | string | `"fp16"` | Model precision: `"f32"` or `"fp16"` |
| `kokoro.speed` | float | `1.0` | Speech rate multiplier (0.5 – 2.0) |
| `kokoro.prewarm` | bool | `false` | Pre-warm model on startup for faster first synthesis |
| `kokoro.data_dir` | string | `""` | Directory for Kokoro model/voices files; empty = `~/.local/share/voxctrl/kokoro/` |

**Example Kokoro config:**

```json
"tts": {
  "enabled": true,
  "engine": "kokoro",
  "voice": "en-us-lessac-medium",
  "voice_dir": "",
  "stop_key": ["KEY_ESCAPE"],
  "response_overlay": true,
  "gpu": true,
  "kokoro": {
    "voice": "af_heart",
    "quality": "fp16",
    "speed": 1.0,
    "prewarm": true,
    "data_dir": ""
  }
}
```

---

## TtsEngineHandle

The TTS handle is stored in `AppState` and shared with the MCP server, routing system, and IPC commands. It uses a robust, generation-based queue cancellation architecture so that calling `stop()` instantly interrupts active audio and safely discards all pending queued utterances without killing the worker thread:

```rust
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,        // None = use config default
    pub source_label: Option<String>, // "prewarm" = suppress audio output
}

pub enum TtsCommand {
    Play {
        utterance: Utterance,
        generation: u32,
    },
    Shutdown,
}

pub struct TtsEngineHandle {
    tx: Sender<TtsCommand>,
    generation: Arc<AtomicU32>,
}

impl TtsEngineHandle {
    pub fn speak(&self, text: impl Into<String>);
    pub fn speak_utterance(&self, u: Utterance);
    pub fn stop(&self);               // Increments generation, interrupts active audio and discards queue
    pub fn shutdown(&self);           // Sends Shutdown command, terminating the worker thread
}
```

The handle is `Clone` — multiple callers can hold a copy and enqueue utterances concurrently.

---

## Kokoro Architecture

```
User speaks → transcription → speak_text IPC
                                    │
                         TtsEngineHandle::speak_utterance()
                                    │
                         (bounded channel, cap 32)
                                    │
                         speak_kokoro()  (pure Rust)
                                    │
              ┌─────────────────────┼──────────────────────┐
              │                     │                       │
    phonemize_espeak()      load_voice_embedding()    ort::Session
    espeak-ng --ipa -q       NPY File → Cached mem     (lazy init,
    (subprocess)             [num_tokens, 256]          cached per
              │                     │                  worker thread)
              │              kokoro_tokenize()               │
              └──────────────────── │ ──────────────────────┘
                                    │
                           run_kokoro_inference()
                           input_ids (int64, 1×T)
                           style     (float32, 1×256)
                           speed     (float32, 1)
                                    │
                           f32 audio samples @ 24 kHz
                                    │
                     i16 PCM → rodio::SamplesBuffer (persistent sink)
                                    │
                           Sink::sleep_until_end()
```
