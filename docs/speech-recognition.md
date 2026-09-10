# Speech Recognition

**Crate:** `crates/voxctrl-inference/`

## Overview

VoxCtrl supports four speech-to-text engine backends to match your hardware and workflow:

1. **`whisper.cpp`**: Reference-grade on-device Whisper transcription (via `whisper-rs`) supporting multi-threaded CPU, Vulkan, and CUDA GPU acceleration.
2. **`Moonshine`**: Streaming ONNX speech recognition tuned for real rooms with ambient background noise; runs efficiently on CPU.
3. **`Parakeet TDT`**: NVIDIA FastConformer TDT 0.6B delivering ultra-fast non-autoregressive transcription with zero repetition loops.
4. **`Remote Speech Engine (Bring Your Own Voice Engine)`**: Offload transcription to any local network or remote speech-to-text service implementing the OpenAI-compatible `/v1/audio/transcriptions` API (e.g. Faster-Whisper-Server, vLLM, LocalAI, Whisper standalone, or cloud providers).

By default, VoxCtrl runs 100% on-device and offline with no data leaving your machine. If you configure a Remote Speech Engine, audio is streamed directly to your designated endpoint.

---

## Model Sizes

| Size | Approx RAM | Speed | Accuracy |
|---|---|---|---|
| `tiny` / `tiny.en` | ~75 MB | Fastest | Lowest |
| `base` / `base.en` | ~142 MB | Fast | Low |
| `small` / `small.en` | ~466 MB | Medium | Medium |
| `medium` / `medium.en` | ~1.5 GB | Slow | High |
| `large-v2` | ~3.1 GB | Slowest | High |
| `large-v3` | ~3.1 GB | Slowest | Highest |
| `large-v3-turbo` | ~1.6 GB | Medium | Near large-v3 |

The `.en` variants are English-only but slightly faster. `large-v3-turbo` is a distilled model offering near large-v3 quality at medium speed.

The default model is **`tiny`**. It's small enough (~75MB) that VoxCtrl downloads it automatically in the background on first launch — no manual step required to start dictating. Models are downloaded from Hugging Face as GGUF files and cached at `~/.local/share/voxctrl/models/` by default; this path is configurable via `engine.whisper_cpp.model_dir`. Larger models (better accuracy, slower) must be downloaded explicitly from Settings → Engine.

Change the active model via `engine.whisper_cpp.model_size` in config. Changing it takes effect on next recording.

---

## Hardware Backends

`engine.whisper_cpp.device` selects the compute backend:

| Value | Description |
|---|---|
| `auto` | Let whisper.cpp use whatever GPU support the build has, falling back to CPU |
| `cuda` | NVIDIA GPU via CUDA *(requires CUDA build — see below)* |
| `vulkan` | Any GPU via Vulkan (AMD/Intel/NVIDIA) |
| `cpu` | Force CPU |

Anything other than `cpu` turns whisper.cpp's GPU path on (`use_gpu`), and whisper.cpp then uses whichever accelerator the binary was built with — CUDA in a `--features cuda` build, Vulkan in the standard one. `auto` and an explicit `cuda`/`vulkan` therefore behave the same; only `cpu` differs.

> **CUDA is opt-in at compile time.** The default build runs on any machine without a GPU. To enable NVIDIA GPU acceleration, build with the `cuda` cargo feature:
> ```bash
> npm run tauri build -- --features cuda
> ```
> The `cuda` option only appears in the Settings → Engine device selector when the binary was compiled with this flag. If a previously saved config specifies `"cuda"` but the running binary is a CPU-only build, the app automatically resets the device to `"auto"` on launch.

---

## Inference Pipeline

When a recording session ends, the accumulated audio buffer is sent to the inference worker thread:

```
InferenceRequest {
    audio: Vec<f32>,           // 16 kHz mono PCM
    target_id: String,         // Which output target (comma-separated for multi-target)
}
```

The worker runs:

```
1. Empty audio check
   └─ audio.is_empty() → return ""

2. Noise gate (VAD)
   └─ rms_threshold = (1.0 - vad_threshold) * 0.006
   └─ rms(audio) < rms_threshold → return ""

3. Build Whisper initial prompt
   └─ Custom vocabulary words from features.custom_vocabulary, appended to the
      standing VoxCtrl preamble

4. whisper-rs transcription
   └─ Reuses pre-allocated WhisperState (KV cache + attention buffers loaded once at startup)
   └─ Returns raw_text with inference_ms and language

5. Post-processing pipeline (in order):
   a. Filler word removal (if enabled)
   b. Spoken punctuation conversion (if enabled)
   c. Auto-format list detection (if enabled)
   d. Snippet expansion (if snippets configured)
   e. Custom vocabulary fuzzy correction
   f. Code mode conversion (if enabled)

6. Silence hallucination filter
   └─ rms < 0.003 AND text is a known Whisper hallucination → return ""

7. Optional LLM post-processing via the OpenAI API (if target.processing.openai_enabled)

8. Return InferenceOutput {
       text: String,            // Final processed text
       raw_text: String,        // Pre-processing Whisper output
       inference_ms: u32,       // Whisper wall time
       language: String,        // Detected language code
       target_id: String,
   }
```

---

## Post-Processing Details

### Filler Word Removal
Enabled via `features.remove_fillers`.

Strips common verbal fillers using a regex with repetition variants:
- `uh`, `um`, `hmm`, `er`, `ah`, `ugh`, `mhm` (e.g. `"uhhh"`, `"umm"` also matched)
- Cleans up resulting double spaces

### Spoken Punctuation Conversion
Enabled via `features.spoken_punctuation`.

Converts spoken words to their symbol equivalents (case-insensitive, word boundaries):

| Spoken | Output | | Spoken | Output |
|---|---|---|---|---|
| "period" / "full stop" | `. ` | | "open bracket" / "open paren" | `(` |
| "comma" | `, ` | | "close bracket" / "close paren" | `)` |
| "question mark" | `? ` | | "new line" | `\n` |
| "exclamation mark" / "exclamation point" | `! ` | | "new paragraph" | `\n\n` |
| "colon" | `: ` | | "tab" | `\t` |
| "semicolon" | `; ` | | "dash" | ` — ` |
| "hyphen" | `-` | | "ellipsis" | `...` |
| "slash" | `/` | | "backslash" | `\` |
| "at sign" | `@` | | "hash" | `#` |
| "percent" | `%` | | "ampersand" | `&` |
| "asterisk" | `*` | | "plus sign" | `+` |
| "equals sign" | `=` | | "less than" | `<` |
| "greater than" | `>` | | | |

### Auto-Format Lists
Enabled via `features.auto_format_lists`.

Detects ordinal pattern words (`first`, `second`, `third`, `fourth`, `fifth`, `finally`, including `firstly`, `secondly`, etc.) and reformats the text as a **numbered list**:

Input: `"First do this then second check that and finally submit"`
Output:
```
1. do this then
2. check that and
3. submit
```

### Snippet Expansion
Enabled whenever `features.snippets` is non-empty.

Short codes in transcribed text are replaced with their expansions (case-insensitive, word boundaries):

```json
"snippets": {
  "addr": "123 Main St, Springfield",
  "sig": "Best regards,\nJane"
}
```

### Custom Vocabulary Correction
Enabled whenever `features.custom_vocabulary` is non-empty.

After transcription, each word is compared against the vocabulary list using **Levenshtein distance fuzzy matching**:

| Word length | Max edit distance allowed |
|---|---|
| 1–3 chars | 0 (exact match only) |
| 4 chars | 1 |
| 5+ chars | 2 |

This corrects Whisper's phonetic approximations of proper nouns, names, and domain-specific terms. Example: vocabulary `["Rufer"]` would correct `"Rufur"` or `"Rupher"` to `"Rufer"`.

### Code Mode
Enabled via target `processing.code_mode = true`.

Converts spoken phrases to code-style syntax:
- Maps spoken operators: `"equals"` → `=`, `"plus"` → `+`, `"minus"` → `-`, `"times"` → `*`, `"divided by"` → `/`, `"modulo"` → `%`
- Converts multi-word lowercase phrases to camelCase: `"my function name"` → `"myFunctionName"`

---

## Silence Hallucination Filter

Whisper generates text like "Thank you." or "Thanks for watching." when given near-silent input. VoxCtrl applies a filter after post-processing:

```
IF rms_energy < 0.003 (absolute room silence)
AND processed_text ∈ ["thank you", "thanks for watching", "thank you for watching"]
THEN discard result → return ""
```

This threshold (0.003 RMS) is intentionally below any genuine speech energy, so saying "thank you" aloud will still be transcribed correctly.

---

## Context Prompting

The Whisper initial prompt is a fixed VoxCtrl preamble plus the
`features.custom_vocabulary` list, formatted as
`"Vocabulary: word1, word2, ..."`. There is no per-target prompt override.

---

## Configuration Options

Under `engine.whisper_cpp` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `model_size` | string | `"tiny"` | Whisper model — `tiny`/`tiny.en` auto-download silently on first launch; larger sizes require an explicit download in Settings → Engine |
| `device` | string | `"auto"` | Compute device |
| `threads` | integer | `0` | CPU threads (0 = auto) |
| `model_dir` | string | `""` | Custom model storage path; empty = `~/.local/share/voxctrl/models/`. Supports `~` expansion (e.g. `~/.whisper-models`). The directory must already exist. |

Language detection is automatic when using whisper-cpp; use the `engine.moonshine.language` field for the Moonshine backend.

## Moonshine backend

[Moonshine](https://github.com/moonshine-ai/moonshine) is an alternative,
CPU-friendly speech-to-text model. Unlike Whisper it consumes the raw 16 kHz
waveform directly (no fixed 30-second window), which keeps latency low on the
short utterances typical of push-to-talk dictation.

It runs through ONNX Runtime as two graphs — an `encoder_model` that turns the
raw waveform into hidden states, and a KV-cached `decoder_model_merged` that
greedily generates tokens: starting from the start-of-transcript token, each
step's highest-scoring token is fed back in (reusing the decoder's attention
cache) until the end-of-transcript token appears or a length cap is reached. The
resulting token ids are turned back into text with the model's tokenizer, which
is bundled into the app.

**Enabling it.** Moonshine is a default build feature, so a standard build
includes it — unlike `cuda`, which stays opt-in. It links ONNX Runtime, fetched
at build time, and shares that runtime with the Inflect-Micro-v2 TTS engine. A
build made with `--no-default-features` omits both and transparently falls back
to whisper-cpp if `"moonshine"` is selected; the Settings → Engine panel
indicates whether the running build actually includes it.

**Models.** Selecting a size (`base` or `tiny`) and clicking Download in
Settings → Engine fetches the two ONNX graphs (`encoder_model.onnx` and
`decoder_model_merged.onnx`) into
`~/.local/share/voxctrl/models/moonshine/<size>/`. You can also drop those two
files there manually to run fully offline; the tokenizer ships inside the app.

---

## Parakeet TDT Backend

[Parakeet TDT](https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3) is an NVIDIA FastConformer transducer model offering state-of-the-art accuracy, real-time transcription speeds, and immune to the autoregressive repetition loops that Whisper can occasionally experience on background noise.

- **Model size**: ~665 MB (INT8 quantized ONNX).
- **Execution**: Runs through ONNX Runtime with optional GPU offloading.
- **Language**: English-focused with automatic language token routing.
- **Storage**: Downloaded into `~/.local/share/voxctrl/models/parakeet/`.

---

## Remote Speech Engine (Bring Your Own Voice Engine)

The **Remote Speech Engine** (`backend = "remote-openai"`) allows you to decouple VoxCtrl's desktop UI and hotkey management from local model inference. Instead of running weights locally, VoxCtrl streams captured audio directly to an external, network-accessible speech-to-text service that implements the OpenAI-compatible `/v1/audio/transcriptions` API.

### Why "Bring Your Own Voice Engine"?

- **Zero Local Footprint**: Requires **0 MB RAM** and **0 MB VRAM** locally. Perfect for battery-constrained laptops, mini-PCs, or development environments where system memory and GPU resources are reserved for compilers, IDEs, or LLMs.
- **Centralized Homelab GPU**: Run high-end speech models (`whisper-large-v3`, `distil-whisper`, or fine-tuned variants) on a dedicated GPU server on your local network (e.g. via [Faster-Whisper-Server](https://github.com/fedirz/faster-whisper-server), [vLLM](https://docs.vllm.ai/), or [LocalAI](https://localai.io/)).
- **Multi-Device Dictation**: Point multiple workstations or laptops running VoxCtrl at a single shared transcription server.
- **Cloud Provider Compatibility**: Connect to hosted speech-to-text APIs (OpenAI Whisper, Groq, Fireworks, Together, or any provider implementing standard `/v1/audio/transcriptions`).

### Configuration Options

Under `engine.remote_openai` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `endpoint` | string | `"http://localhost:8000/v1"` | Base URL or full endpoint path (e.g. `http://192.168.1.50:8000/v1` or `https://api.openai.com/v1`). |
| `api_key` | string or null | `null` | Optional Bearer authentication token. Leave blank for unauthenticated LAN servers. |
| `model` | string | `"whisper-1"` | Model identifier expected by the server (e.g. `whisper-1`, `Systran/faster-whisper-large-v3`, `large-v3`). |
| `language` | string | `""` | Optional ISO language code (e.g. `en`, `es`, `fr`, or blank for automatic detection). |
| `timeout_secs` | integer | `30` | Request timeout in seconds (5–300). |

### Connection Testing & Model Discovery

Both the **Onboarding Wizard** and **Settings → Engine** include an interactive **Test Connection** button:
- **Ping & Auth Verification**: Submits a sample audio test payload to the configured endpoint.
- **Model Discovery**: Queries `/v1/models` on the server and surfaces discovered models as clickable tag chips, allowing you to select models directly from the server's catalog.
- **Visual Feedback**: Displays connection latency, model readiness, and clear diagnostics on failure (connection refused, 401 unauthorized, timeouts).
- **Onboarding Gate**: When selected during first-run setup, the wizard requires a successful test before proceeding to guarantee working dictation.

### How It Works Under the Hood

When you press your dictation hotkey:
1. Audio is recorded and VAD-filtered using VoxCtrl's low-latency capture pipeline.
2. The audio buffer is packaged as a standard 16 kHz 16-bit mono WAV payload.
3. An asynchronous HTTP `POST` multipart request is dispatched to `{endpoint}/audio/transcriptions` (or `{endpoint}` if the path already ends with `/audio/transcriptions`).
4. The server's JSON transcription response (`{ "text": "..." }`) is parsed and passed straight into VoxCtrl's post-processing pipeline (filler removal, punctuation conversion, snippet expansion, custom vocabulary, and output routing).

---

## STT Engine Comparison

| Feature | `whisper.cpp` | `Moonshine` | `Parakeet TDT` | `Remote Speech Engine` |
|---|---|---|---|---|
| **Location** | 100% On-device | 100% On-device | 100% On-device | Network / Server |
| **Local RAM** | 75 MB – 3.1 GB | ~240 – 530 MB | ~665 MB | **~0 MB** |
| **Local VRAM** | 0 – 3.5 GB (CUDA/Vulkan) | 0 MB (CPU native) | 0 – 1 GB (optional) | **0 MB** |
| **Speed** | Fast (GPU) / Medium (CPU) | Fast (CPU) | Ultra-fast (non-autoregressive) | Network + Server speed |
| **Quiet Room Accuracy** | Reference-grade (0.97) | High (0.77) | SOTA (0.94) | Server-model dependent |
| **Noisy Room Accuracy** | Moderate | Very high (0.93 retention) | High (0.88 retention) | Server-model dependent |
| **Repetition Loops** | Possible on silent/noisy audio | Rare | None (TDT alignment) | Server-model dependent |
| **Offline Operation** | Yes | Yes | Yes | LAN / Internet |

