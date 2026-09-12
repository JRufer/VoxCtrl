# Text-to-Speech

**Crate:** `crates/voxctrl-tts/`

## Overview

VoxCtrl includes a neural TTS engine for voice output. This is useful for reading back transcriptions, confirming commands, or building conversational voice interactions via the MCP server.

---

## Engines

### Breeze-TTS-2 (Neural, Voice Design)
[Breeze-TTS-2](https://huggingface.co/BreezeBlue/Breeze-TTS-2) is an open-weight, bilingual (English/Chinese) speech generation model by BreezeBlue, designed specifically for ultra-low latency real-time interaction.

Key capabilities include **Voice Design**, which generates voices from natural-language descriptions without requiring reference audio clips (e.g. *"A calm female voice speaking clearly with a gentle tone"*).

> **License & Responsible Use Warning:**
> Breeze-TTS-2 model weights are released under the **BreezeBlue Research and Non-Commercial License**. Commercial use requires separate written authorization from RESONIA, INC.

**Features & Optimization:**
- **Voice Design Prompts:** Set `speaker_prompt` in Settings to describe the desired voice characteristics.
- **HuggingFace Access Token:** The gated model weights require a HuggingFace access token, stored once as `tts.hf_token` and shared by Pocket-TTS and Breeze-TTS-2. The setup wizard asks for it on the voice step, and Settings → TTS edits the same value. An `HF_TOKEN` exported into the environment takes precedence: it is shown in both fields read-only and is never written to the config.
- **Prewarming:** Enables startup prewarming to load weights into VRAM so the first synthesis is instant.
- **GPU Acceleration:** CUDA (NVIDIA) or Metal (macOS) offload can be enabled for near real-time response speeds, in a build that includes the matching feature. See [GPU Acceleration](#gpu-acceleration).

### Piper (Primary)
[Piper](https://github.com/rhasspy/piper) is a fast, local neural TTS system using ONNX models. It produces high-quality natural-sounding speech entirely offline.

VoxCtrl invokes the `piper` binary directly (looks first in `~/.local/share/voxctrl/piper/piper`, then on PATH). It pipes text to Piper's stdin, receives raw 16-bit PCM on stdout, and plays via rodio (cross-platform).

### Pocket-TTS (Neural, Voice Cloning)
[Pocket-TTS](https://github.com/kyutai-labs/pocket-tts) is Kyutai's lightweight FlowLM + Mimi-codec TTS model, ported to pure Rust on top of [Candle](https://github.com/huggingface/candle). VoxCtrl uses the [`pocket-tts`](https://crates.io/crates/pocket-tts) crate directly — no Python, no ONNX, no subprocess.

Instead of fixed precomputed voice embeddings, Pocket-TTS clones a voice from a short reference audio clip at runtime (`TTSModel::get_voice_state()`). VoxCtrl ships a small built-in catalogue of reference clips so users get a normal voice-picker UX without needing to record anything themselves.

**Prerequisites:**
- A HuggingFace account that has accepted the license for the gated [`kyutai/pocket-tts`](https://huggingface.co/kyutai/pocket-tts) model repo, and a personal access token with read access. Set it as `tts.hf_token` — in the setup wizard's voice step, or in Settings → TTS — or export it as `HF_TOKEN`, which wins over the saved one.

Model weights and the per-voice reference clips are downloaded on demand via `pocket_tts::weights::download_if_necessary`, which resolves `hf://owner/repo/filename[@revision]` URIs through the standard HuggingFace cache (`~/.cache/huggingface/hub/`). Subsequent loads are read straight from the local cache — no network access required once downloaded.

### Inflect-Micro-v2 (Neural, ONNX)
[Inflect-Micro-v2](https://huggingface.co/owensong/Inflect-Micro-v2) is a ~9.4M-parameter VITS-family text-to-waveform model (37.5 MB FP32, Apache 2.0) producing 24 kHz mono audio from a single fixed English voice. It is the smallest neural option VoxCtrl offers and runs in-process through ONNX Runtime with no subprocess.

The verified FP32 export is published separately as [`Inflect-Micro-v2-ONNX`](https://huggingface.co/owensong/Inflect-Micro-v2-ONNX), with the graphs under `onnx/`. VoxCtrl lists the repository through the Hugging Face API and downloads the graphs plus their accompanying files into `~/.local/share/voxctrl/models/inflect-micro/`. Point `model_dir` at an existing copy to skip downloading.

**Pipeline**, following the export's own `inference_onnx.py`:

1. `duration.onnx` — `tokens` (int64 `[1, N]`), `lengths` (int64 `[1]`), `length_scale` (float32 scalar) → `m_p_exp`, `logs_p_exp`, `y_mask`
2. Latent noise `zp_noise` is drawn **host-side** with `m_p_exp`'s shape
3. `decode.onnx` — `m_p_exp`, `logs_p_exp`, `y_mask`, `zp_noise`, `noise_scale` (float32 scalar) → `waveform`

`length_scale` is `1.0 / speed`; `noise_scale` is the variation setting (0.0–1.0, default 0.667). Both scale inputs are rank-0 (scalar) tensors, matching `np.asarray(value, dtype=np.float32)` in the reference.

**Tokenization.** Phoneme ids are positions in the ordered 178-entry `symbols` list from the model's text frontend (`text/symbols.py`), which follows the tacotron layout: a pad, then punctuation, ASCII letters, and the IPA inventory. Ids are interleaved with blanks into `[0, s₀, 0, s₁, …, 0]` (length `2n+1`); there is no BOS/EOS wrapper. `'` appears twice in the list and, as in Python's dict comprehension, the later index wins.

That list is published in the PyTorch repository rather than with the graphs, so VoxCtrl fetches it separately after the download. The loader parses the tacotron form (`symbols = [_pad] + list(_punctuation) + ...`, resolving the named constants) as well as plain list literals, JSON arrays, and symbol→id maps — and identifies the table by parsing rather than by filename, so it does not depend on a fixed name. eSpeak-NG's `en-us --ipa` output is fully covered by this inventory; any symbol that ever falls outside it is skipped with a warning rather than failing the utterance.

**Assets.** The graphs are found by listing the hub API rather than assuming a path, and the phoneme table is identified by parsing candidate files rather than by filename — both had to be discovered, since the export publishes the graphs and the symbol list in different repositories and ships no standalone table.

**Chunking.** Text is normalised, split after `.!?;:` followed by whitespace, and any sentence over 280 characters is split again at the last `,`/`;`/`:` in range (or the last space). Each sentence is its own chunk — they are not packed together — so the per-chunk boundary pause (0.28 s after `?`, 0.22 s after `.`, down to 0.08 s with no terminator) lands correctly. Every chunk gets a 5 ms edge fade, and the seed advances per chunk. Playback of each chunk overlaps generation of the next.

**Seed reproducibility.** The reference draws `zp_noise` with NumPy's PCG64. VoxCtrl uses its own PCG64 with Box–Muller instead, so output is fully deterministic for a given seed *within VoxCtrl*, but a seed does not select the same sample as the same seed in the Python reference. Any correctly-distributed noise produces valid audio; the seed only chooses which sample you get.

**Prerequisites:**

- Nothing, in a standard build: `inflect-micro` is a default feature, so ONNX
  Runtime is fetched at build time and linked in (the `download-binaries` setup
  `voxctrl-inference` uses), and the resulting binary needs no system
  `libonnxruntime`. Building therefore needs network access to the ONNX Runtime
  binary host; `--no-default-features --features custom-protocol` builds offline
  without this engine or Moonshine.

  In a build without the feature the model still **downloads** — only synthesis is
  gated — so Settings shows the engine as ready while Test TTS stays disabled and
  explains why.

- `espeak-ng` installed on the system, used for grapheme-to-phoneme conversion.

Graph signatures are verified at load, and a missing input is a hard error reporting what the graph actually declares. Settings → TTS → Inspect graphs (or the `inflect_micro_inspect` command) reports that signature for a downloaded model.

**Debugging.** When synthesis misbehaves, run the pipeline outside the app:

```bash
cargo run -p voxctrl-tts --features inflect-micro --example inflect_probe
cargo run -p voxctrl-tts --features inflect-micro --example inflect_probe -- "custom text"
```

It runs phonemization → tokenization → both graphs and writes `inflect_probe.wav`
to the temp directory, with no Tauri, no event plumbing and no audio device. Each
stage prints before and after it runs, so a stall or crash leaves the responsible
stage as the last line on screen. Because it writes a WAV instead of playing it,
it also separates a synthesis fault from a playback one — something the app's
Test button cannot distinguish.

### VoxCPM2 (Neural, Voice Design & Cloning)
[VoxCPM2](https://huggingface.co/openbmb/VoxCPM2) is an open-source speech generation model by OpenBMB released under the **Apache-2.0 License**. It is ported to pure Rust on top of Candle and generates rich 24 kHz mono audio.

**Key Features:**
- **Voice Design**: Generate speech using a natural-language description of the speaker voice (`speaker_prompt`), e.g. *"A calm young female voice speaking clearly with a gentle tone."*
- **Voice Cloning**: Clone a voice using reference `.wav` audio clips stored in the shared voices directory (`~/.local/share/voxctrl/pocket-tts-voices/`).
- **Ultimate Cloning**: When paired reference audio and matching transcript files exist in the same directory, VoxCPM2 enables high-fidelity cloned synthesis.
- **Pure Rust Engine**: Runs natively via Candle without Python or external subprocesses.
- **Model Storage**: Model assets (`config.json`, `generation_config.json`, weights) are stored in `~/.local/share/voxctrl/models/voxcpm2/` (configurable via `tts.vox_cpm_2.model_dir`).
- **GPU Acceleration**: Optional CUDA acceleration (`tts.vox_cpm_2.gpu`) with seamless CPU fallback.

### Espeak-ng (Lightweight)
If Piper is unavailable or no voice is downloaded, VoxCtrl can use `espeak-ng`. It is invoked as a subprocess with the text as an argument. Quality is lower but espeak-ng is always available as a system package.

---

## GPU Acceleration

Three engines can run on the GPU, each through its own mechanism:

*   **Piper** (`tts.gpu`): appends the `--cuda` CLI flag to the spawned `piper`
    subprocess at runtime. Needs the app built with the `cuda` feature.
*   **Breeze-TTS-2** (`tts.breeze_tts_2.gpu`): loads the model onto a candle GPU
    device instead of the CPU. Needs the app built with `breeze-cuda` (NVIDIA) or
    `breeze-metal` (macOS) on `voxctrl-tts`.
*   **VoxCPM2** (`tts.vox_cpm_2.gpu`): loads the model onto a candle GPU (CUDA)
    device instead of the CPU.

Breeze-TTS-2 and VoxCPM2 run on candle, whose only GPU backends are CUDA
and Metal — **there is no Vulkan path** to select, on any platform. A build
without one of those features logs a warning when the setting is on and
synthesizes on the CPU.

Pocket-TTS shares Breeze's runtime but exposes no GPU toggle of its own, and
Inflect-Micro-v2 runs on ONNX Runtime's CPU provider. Inflect is small enough
(9.4M parameters) that CPU synthesis is fast; the upstream export also supports
CUDA and DirectML providers, which VoxCtrl does not currently select.

### Requirements & Setup:
1.  A CUDA-compatible NVIDIA GPU and drivers (or an Apple Silicon Mac, for Metal).
2.  Build with the feature for the engine you want:
    ```bash
    cargo tauri dev --features cuda           # Piper
    cargo tauri dev --features breeze-cuda    # Breeze-TTS-2 on NVIDIA
    cargo tauri dev --features breeze-metal   # Breeze-TTS-2 on macOS
    ```
    If GPU initialization fails, both engines fall back to the **CPU** without
    crashing — Breeze logs the reason and reloads on the CPU.

Toggling the Breeze GPU setting reloads the model: the device is fixed when the
weights are placed, so the next utterance after the change pays a load, and
subsequent ones do not.

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

### Pocket-TTS Voices

VoxCtrl bundles a small catalogue of reference voice clips, each pulled from the public (ungated) [`kyutai/tts-voices`](https://huggingface.co/datasets/kyutai/tts-voices) dataset repo via an `hf://` URI. Switching voices triggers a one-time download and embedding of that voice's reference clip (`TTSModel::get_voice_state()`), then the computed `ModelState` is cached in memory for the life of the worker thread.

| ID | Name |
|---|---|
| `alba` | Alba (Female, default) |
| `anna` | Anna (Female) |
| `vera` | Vera (Female) |
| `charles` | Charles (Male) |
| `michael` | Michael (Male) |

### Inflect-Micro-v2 Voices

None — the model has a single fixed English voice, so Settings shows a seed and a variation control instead of a voice picker. Changing the seed resamples the delivery of the same voice; it does not select a different speaker.

### Custom Pocket-TTS Voices

Drop a `.wav` reference clip into the configured `pocket_tts.voice_dir` (default `~/.local/share/voxctrl/pocket-tts-voices/`) to add it to the voice list — no re-encoding or extra metadata needed:

- The filename (without extension) becomes the voice's id, e.g. `narrator.wav` adds a voice listed as "Narrator (Custom)".
- Naming a clip after a built-in voice (e.g. `alba.wav`) overrides that voice's bundled reference clip instead of adding a new entry.
- Any sample rate works — `TTSModel::get_voice_state()` resamples to the model's rate automatically.
- Custom clips are read directly from disk; they don't go through the HuggingFace cache or require `download_pocket_tts`.

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

### Pocket-TTS — Checking and downloading

Pocket-TTS downloads the model weights (from the gated `kyutai/pocket-tts` repo), the tokenizer, and the selected voice's reference clip — all resolved through the HuggingFace cache.

```typescript
// Check if model weights, tokenizer, and the selected voice's reference clip are cached
const ready = await invoke<boolean>('check_pocket_tts_ready', {
  voice: 'alba',
  voiceDir: '',           // '' = default custom-voice directory
});

// Download model weights, tokenizer, and the reference clip (requires hf_token)
// No-op for custom voices resolved from voiceDir — they're already on disk.
await invoke('download_pocket_tts', {
  voice: 'alba',
  voiceDir: '',
  hfToken: '<your HuggingFace token>',
});

// List the merged catalogue (built-ins + any .wav files found in voiceDir)
const voices = await invoke<{ id: string; label: string }[]>('list_pocket_tts_voices', {
  voiceDir: '',
});
```

---

## Audio Playback

After synthesis, audio is played using `rodio` (cross-platform):

- **Piper** produces raw 16-bit signed LE PCM; rodio plays it directly via `SamplesBuffer`.
- **Pocket-TTS** is generated and played frame-by-frame via `TTSModel::generate_stream()` rather than waiting for the whole utterance: each Mimi audio frame (`candle::Tensor`) is converted to i16 PCM with `pocket_tts::audio::pcm_i16_le_bytes()` and appended to the sink as soon as it's ready, played via `SamplesBuffer` at 24 kHz. This overlaps playback of earlier frames with generation of later ones, cutting perceived latency from "time to generate the whole sentence" down to roughly "time to generate the first frame." `stop()` is checked between frames, so playback can be interrupted mid-generation instead of only after the whole utterance finishes.

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
// For Pocket-TTS the voice parameter overrides cfg.tts.pocket_tts.voice:
await invoke('speak_text', { text: 'Hello world', voice: 'charles' });
```

The `voice` parameter is optional; if omitted, the configured default voice is used.

### From a FIFO Response Pipe
If a target has a `response_pipe` path configured, VoxCtrl watches that FIFO for newline-terminated text and speaks each line:

```bash
echo "Recording started" > /tmp/voxctrl-tts.fifo
```

---

## Pre-warming Pocket-TTS

Pocket-TTS loads its model weights on first synthesis. Enable `prewarm` to avoid this latency:

```json
"tts": {
  "engine": "pocket_tts",
  "pocket_tts": { "prewarm": true }
}
```

When `prewarm` is `true`, `TtsEngineWorker::start()` enqueues a silent synthesis immediately after spawning the worker thread. The worker processes this short request (a single space) at startup, loading the model into memory and computing the configured voice's reference embedding. Subsequent user-triggered syntheses are faster because the model is already warm. This adds startup latency depending on model size and disk speed.

Pre-warming is ignored when `memory_mode` is `"on_demand"` — the two settings want opposite things, and the memory mode wins. The same applies to `breeze_tts_2.prewarm` and `inflect_micro.prewarm`.

---

## Model Memory (on-demand loading)

The neural engines — Pocket-TTS, Breeze-TTS-2, VoxCPM2, and Inflect-Micro-v2 — are the only parts of the
TTS stack that occupy significant memory; Piper and eSpeak-NG run a process per utterance and
hold nothing in between. `memory_mode` decides whether those weights stay resident:

```json
"tts": {
  "engine": "pocket_tts",
  "memory_mode": "on_demand",
  "idle_unload_secs": 900
}
```

| Mode | Behaviour |
|---|---|
| `"always_loaded"` (default) | The model is loaded on first use and stays resident for the life of the TTS worker. Fastest, highest memory. |
| `"on_demand"` | The model is loaded when VoxCtrl knows it is needed, kept primed while it keeps being used, and dropped after `idle_unload_secs` (default 900 = 15 minutes) of inactivity. |

In `"on_demand"` mode TTS itself stays enabled the whole time — only the weights come and go:

* **Loading starts as early as possible.** `TtsCommand::Preload` is sent the moment a recording
  starts against a target that ends in speech (a `speak` target, or one with a `response_pipe`),
  so the model loads while the user is still talking rather than after they stop. Dictating into
  an ordinary injection target does not load it.
* **The idle countdown restarts on every use.** Speaking an utterance or pre-loading stamps the
  worker's `last_used`, so a back-and-forth conversation never reloads mid-flow. A long utterance
  does not count against the window either — the clock restarts when playback ends.
* **Unloading drops the model and every cached voice state**, which is the whole of the resident
  footprint. The worker thread, its audio device, and the utterance queue stay up, so nothing
  else about TTS changes.
* **The floor is 30 seconds.** A shorter `idle_unload_secs` is clamped, since dropping the model
  between two sentences of the same reply would cost far more than it saves.

The mode is settable in **Settings → TTS → Model Memory** and from the **VoxCtrl tray icon**
("Unload TTS model when idle"), which toggles it live — the running worker is told about the new
policy (through `TtsCommand::UpdateConfig`) rather than restarted, so playback and the audio
device are untouched.

---

## Stopping Playback

The `stop_key` config field lists keys that interrupt current TTS playback when pressed:

```json
"tts": {
  "stop_key": ["KEY_ESCAPE"]
}
```

Sending `None` through the TTS engine channel (via `TtsEngineHandle::stop()`) clears the current utterance.

**Starting dictation also stops playback.** Every path that begins capture — the hotkey gesture, the settings window, the D-Bus service behind a native desktop shortcut, and MCP voice capture — goes through `AppState::begin_recording`, which interrupts the current response first. Talking over a spoken reply means interrupting it, and recording while the speakers are still going would feed VoxCtrl's own voice back into the microphone.

**Escape and the desktop portal.** The default stop key is Escape, and it works as it reads: press it and playback stops. How VoxCtrl registers it depends on the backend. Where VoxCtrl watches the key stream itself (X11, evdev, the Windows hook) nothing is grabbed, every app still receives Escape, and the binding is simply registered for the whole session. Where the desktop owns the grab (the XDG `GlobalShortcuts` portal), a standing registration would be *exclusive* — no other app would see Escape while VoxCtrl ran — so VoxCtrl holds it only while it is speaking and gives it back two seconds after playback ends. A stop key with a modifier (`Ctrl+Escape`) is an ordinary shortcut everywhere and is held throughout. See [hotkeys.md](hotkeys.md#bare-escape-and-the-exclusive-grab).

---

## Configuration Options

Under `tts` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `enabled` | bool | `false` | Enable TTS functionality |
| `engine` | string | `"espeak"` | `"piper"`, `"pocket_tts"`, `"inflect_micro"`, `"breeze_tts_2"`, `"vox_cpm_2"`, or `"espeak"`. eSpeak-NG is the default because it's a system package with no model download; the others need a voice/model download first. |
| `voice` | string | `"en-us-lessac-medium"` | Default voice for Piper (hyphen-delimited) |
| `voice_dir` | string | `""` | Directory for Piper voice files; empty = `~/.local/share/voxctrl/piper-voices/` |
| `stop_key` | string[] | `["KEY_ESCAPE"]` | Keys that interrupt playback |
| `response_overlay` | bool | `true` | Show overlay indicator while TTS is speaking |
| `gpu` | bool | `false` | Enable GPU acceleration (CUDA) for Piper. Breeze-TTS-2 and VoxCPM2 have their own GPU settings |
| `hf_token` | string or null | `null` | The single HuggingFace access token used to download every gated model (Pocket-TTS and Breeze-TTS-2). An exported `HF_TOKEN` takes precedence and is never saved here. A config written when each engine held its own copy is migrated to this key on load |
| `pocket_tts.voice` | string | `"alba"` | Default Pocket-TTS voice ID: `"alba"`, `"anna"`, `"vera"`, `"charles"`, `"michael"` |
| `pocket_tts.prewarm` | bool | `false` | Pre-warm model on startup for faster first synthesis |
| `pocket_tts.voice_dir` | string | `""` | Directory scanned for custom `.wav` voice clips; empty = `~/.local/share/voxctrl/pocket-tts-voices/` |
| `breeze_tts_2.voice_mode` | string | `"prompt"` | `"prompt"` for Voice Design, `"clone"` to use a reference clip |
| `breeze_tts_2.speaker_prompt` | string | *(a calm, clear female voice)* | Natural-language description of the speaker, used in `"prompt"` mode |
| `breeze_tts_2.cloned_voice` | string | `"alba"` | Voice id from the shared clip folder, used in `"clone"` mode |
| `breeze_tts_2.voice_dir` | string | `""` | Shared with Pocket-TTS; empty = `~/.local/share/voxctrl/pocket-tts-voices/` |
| `breeze_tts_2.model_dir` | string | `""` | Model weights & tokenizer; empty = `~/.local/share/voxctrl/models/breeze-tts-2/` |
| `breeze_tts_2.prewarm` | bool | `false` | Pre-warm the model on startup for faster first synthesis |
| `breeze_tts_2.gpu` | bool | `false` | Run synthesis on the GPU; needs a `breeze-cuda` / `breeze-metal` build, CPU otherwise |
| `vox_cpm_2.voice_mode` | string | `"prompt"` | `"prompt"` for Voice Design, `"clone"` for reference voice clip |
| `vox_cpm_2.speaker_prompt` | string | *(calm young female voice)* | Natural-language voice description for Voice Design |
| `vox_cpm_2.cloned_voice` | string | `"alba"` | Voice ID from the shared voice clip folder for cloning |
| `vox_cpm_2.voice_dir` | string | `""` | Directory scanned for custom reference clips; empty = platform default |
| `vox_cpm_2.ultimate_cloning` | bool | `false` | Enable Ultimate Cloning when paired audio + transcript files are provided |
| `vox_cpm_2.model_dir` | string | `""` | Directory holding model weights; empty = `~/.local/share/voxctrl/models/voxcpm2/` |
| `vox_cpm_2.prewarm` | bool | `false` | Pre-warm model on startup for faster first synthesis |
| `vox_cpm_2.gpu` | bool | `false` | Enable CUDA GPU acceleration |
| `memory_mode` | string | `"always_loaded"` | `"always_loaded"` or `"on_demand"` — see [Model Memory](#model-memory-on-demand-loading) |
| `idle_unload_secs` | int | `900` | Idle seconds before the model is unloaded in `"on_demand"` mode (minimum 30) |
| `snippets` | object | *(VoxCtrl pronunciations)* | Word → spoken expansion map, applied to speech only |

**Example Pocket-TTS config:**

```json
"tts": {
  "enabled": true,
  "engine": "pocket_tts",
  "voice": "en-us-lessac-medium",
  "voice_dir": "",
  "stop_key": ["KEY_ESCAPE"],
  "response_overlay": true,
  "gpu": false,
  "hf_token": "hf_...",
  "pocket_tts": {
    "voice": "alba",
    "prewarm": true,
    "voice_dir": ""
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
    UpdateConfig(TtsConfig), // live config swap, including the memory mode
    Preload,                 // load the model now, without speaking
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

## Pocket-TTS Architecture

```
User speaks → transcription → speak_text IPC
                                    │
                         TtsEngineHandle::speak_utterance()
                                    │
                         (bounded channel, cap 32)
                                    │
                         speak_pocket_tts()  (pure Rust)
                                    │
              ┌─────────────────────┴──────────────────────┐
              │                                             │
    pocket_tts::TTSModel::load()                  download_if_necessary()
    (lazy init, cached per                         resolves hf://owner/repo/file
     worker thread)                                via HF cache (gated repo,
              │                                     needs HF_TOKEN on first pull)
              │                                             │
              │                              TTSModel::get_voice_state(clip_path)
              │                              → ModelState (cached per voice id)
              └────────────────────┬────────────────────────┘
                                    │
                       TTSModel::generate(text, voice_state)
                                    │
                         candle::Tensor audio samples @ 24 kHz
                                    │
              pocket_tts::audio::pcm_i16_le_bytes() → i16 PCM
                                    │
                     rodio::SamplesBuffer (persistent sink)
                                    │
                           Sink::sleep_until_end()
```
