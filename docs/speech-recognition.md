# Speech Recognition

**Crate:** `crates/voxctr-inference/`

## Overview

VoxCtr uses **Whisper** (via `whisper-rs`, native bindings to whisper.cpp) for speech-to-text. Whisper runs entirely on-device using downloaded GGUF model files. No audio is ever sent to a remote server.

---

## Model Sizes

| Size | VRAM / RAM | Speed | Accuracy |
|---|---|---|---|
| `tiny` | ~75 MB | Fastest | Lowest |
| `base` | ~142 MB | Fast | Low |
| `small` | ~466 MB | Medium | Medium |
| `medium` | ~1.5 GB | Slow | High |
| `large-v3` | ~3.1 GB | Slowest | Highest |

Models are downloaded from Hugging Face as GGUF files on first use and cached at `~/.local/share/voxctl/models/`.

The active model is set via `engine.model_size` in config. Changing it triggers a model reload on next recording.

---

## Hardware Backends

`engine.device` selects the compute backend:

| Value | Description |
|---|---|
| `auto` | Auto-detect: CUDA → Vulkan → CPU |
| `cuda` | NVIDIA GPU via CUDA |
| `vulkan` | Any GPU via Vulkan (AMD/Intel/NVIDIA) |
| `cpu` | Force CPU |

On startup, VoxCtr probes for CUDA/Vulkan availability and logs the selected backend. CPU fallback is always available.

---

## Inference Pipeline

When a recording session ends (hotkey released or VAD timeout), the accumulated audio buffer is sent to the inference worker:

```
InferenceRequest {
    audio: Vec<f32>,     // 16 kHz mono PCM
    target_id: String,   // Which output target to use
    context_text: Option<String>, // AT-SPI context for better transcription
}
```

The worker thread runs:

```
1. Noise gate check
   └─ rms(audio) < vad_threshold → return ""

2. Silence hallucination filter
   └─ rms < 0.003 AND text contains "Thank you" → return ""

3. whisper-rs transcription
   └─ Returns raw text with timestamps

4. Post-processing pipeline (in order):
   a. Filler word removal
   b. Spoken punctuation conversion
   c. Auto-format (list detection)
   d. Snippet expansion
   e. Ollama LLM rewrite (optional)

5. Return InferenceOutput {
       text: String,          // Final processed text
       raw_text: String,      // Pre-processing text
       inference_ms: u32,     // Whisper wall time in ms
       language: String,      // Detected language
       target_id: String,
   }
```

---

## Post-Processing Details

### Filler Word Removal
Enabled via `features.remove_fillers`.

Strips common verbal fillers using regex:
- `um`, `uh`, `hmm` (case-insensitive, at word boundaries)
- Cleans up resulting double spaces

### Spoken Punctuation Conversion
Always applied. Converts spoken words to punctuation marks:

| Spoken | Output |
|---|---|
| "period" / "full stop" | `.` |
| "comma" | `,` |
| "question mark" | `?` |
| "exclamation mark" | `!` |
| "colon" | `:` |
| "semicolon" | `;` |
| "new line" / "newline" | `\n` |
| "new paragraph" | `\n\n` |
| "open paren" | `(` |
| "close paren" | `)` |
| "dash" | `-` |

### Snippet Expansion
Enabled via `features.snippets` (a key-value map in config).

Short codes in transcribed text are replaced with their expansions before delivery:

```json
"snippets": {
  "addr": "123 Main St, Springfield",
  "sig": "Best regards,\nJane"
}
```

### Auto-Format (List Mode)
When transcription contains list-like patterns (lines starting with numbers or dashes), VoxCtr can reformat them as a clean bullet list.

### Code Mode
When an output target has `processing.code_mode = true`, the post-processor preserves camelCase, underscores, brackets, and other programming tokens that Whisper might otherwise alter.

### Ollama Integration
When `ollama.enabled = true`, the transcribed text is sent to a local Ollama endpoint for rewriting. See [Integrations → Ollama](./integrations.md#ollama) for details.

---

## Silence Hallucination Filter

Whisper has a known behavior where it generates text like "Thank you." or "Thank you for watching." when given near-silent input. VoxCtr applies a heuristic filter:

```
IF rms_energy < 0.003
AND result_text ∈ ["Thank you.", "Thank you for watching.", ...]
THEN discard result
```

This prevents phantom text injection when accidentally holding the hotkey in silence.

---

## Context Prompting (AT-SPI)

When `atspi.context_prompt = true`, VoxCtr reads the currently focused text field using AT-SPI2 (Linux accessibility API) and includes the last ~200 characters as a Whisper context prompt. This improves transcription continuity and helps Whisper maintain consistent formatting and vocabulary with the surrounding text.

---

## Configuration Options

Under `engine` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `backend` | `string` | `"whisper"` | Recognition backend (`"whisper"` or `"moonshine"`) |
| `model_size` | `string` | `"base"` | Whisper model: tiny/base/small/medium/large-v3 |
| `device` | `string` | `"auto"` | Compute device: auto/cpu/cuda/vulkan |
| `language` | `string` | `"en"` | Language code or `"auto"` for detection |
