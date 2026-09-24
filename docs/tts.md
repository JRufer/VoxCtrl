# Text-to-Speech

**Crate:** `crates/voxctrl-tts/`

## Overview

VoxCtrl includes a neural TTS engine for voice output. This is useful for reading back transcriptions, confirming commands, or building conversational voice interactions via the MCP server.

Three of the neural engines — Pocket-TTS, Breeze-TTS-2, and VoxCPM2 — run through
[audio.cpp](https://github.com/0xShug0/audio.cpp), a ggml-based, Apache-2.0-licensed
C++ inference engine with a Vulkan GPU backend. VoxCtrl downloads audio.cpp's
prebuilt binaries — no C++ toolchain, no Python, and no proprietary
dependencies (this replaced an earlier pure-Rust/Candle implementation that
pulled in `intel-mkl-src`, a proprietary-licensed Intel MKL redistribution
incompatible with VoxCtrl's MIT license).

Synthesis runs through a long-lived `audiocpp_server` process per engine
(`crates/voxctrl-tts/src/audiocpp.rs`, `AudioCppSession`) rather than a fresh
`audiocpp_cli` spawn per utterance: loading a multi-gigabyte GGUF model
dominates a one-shot call's wall time (measured on Breeze-TTS-2: ~4s of a
~5s request was model load, ~1s was generation), so a resident session is the
difference between a multi-second and a sub-two-second reply. Requests go
over HTTP (`POST /v1/audio/speech`) to a `127.0.0.1` port VoxCtrl picks at
spawn time. This session is exactly what [Model Memory](#model-memory-on-demand-loading)
manages: dropping it (idle-unload, an engine switch, or a GPU-setting change)
kills the child process outright.

---

## Engines

### Breeze-TTS-2 (Neural, Voice Cloning)
[Breeze-TTS-2](https://huggingface.co/BreezeBlue/Breeze-TTS-2) is an open-weight, bilingual (English/Chinese) speech generation model by BreezeBlue, designed specifically for ultra-low latency real-time interaction. VoxCtrl runs it through audio.cpp's `breeze_tts` model family.

> **Known limitation (audio.cpp v0.8.0):** the model family also advertises **Voice Design**
> (natural-language prompts, no reference clip needed — passed as `request_options.instruction`,
> the family's own request-option name; the generic `--instruct`/`instruct` field does not work
> for `breeze_tts`). In practice it does not reliably apply the described voice and the output
> quality suffers, so the UI only offers Voice Cloning for this engine — `speaker_prompt` and
> `voice_mode: "prompt"` remain in the config for compatibility but are not reachable from
> Settings or the setup wizard.

Voice Cloning for Breeze-TTS-2 additionally requires a matching transcript: audio.cpp rejects a clone request with no `--reference-text`, so a `.txt` file alongside the `.wav` reference clip is mandatory here (optional for Pocket-TTS/VoxCPM2).

> **License & Responsible Use Warning:**
> Breeze-TTS-2 model weights are released under the **BreezeBlue Research and Non-Commercial License**. Commercial use requires separate written authorization from the model's publisher. Settings and the setup wizard both show this warning before you download the model.

**Features & Optimization:**
- **Model download:** The GGUF package is hosted on the (ungated) `audio-cpp/audio.cpp-gguf` mirror — no HuggingFace token required.
- **GPU Acceleration:** Vulkan offload can be enabled in Settings (`tts.breeze_tts_2.gpu`); synthesis falls back to the CPU whenever no usable Vulkan device is found. See [GPU Acceleration](#gpu-acceleration).

### Piper (Primary)
[Piper](https://github.com/rhasspy/piper) is a fast, local neural TTS system using ONNX models. It produces high-quality natural-sounding speech entirely offline.

VoxCtrl invokes the `piper` binary directly (looks first in `~/.local/share/voxctrl/piper/piper`, then on PATH). It pipes text to Piper's stdin, receives raw 16-bit PCM on stdout, and plays via rodio (cross-platform).

### Pocket-TTS (Neural, Voice Cloning)
[Pocket-TTS](https://huggingface.co/kyutai/pocket-tts) is Kyutai's lightweight FlowLM + Mimi-codec TTS model. VoxCtrl runs it through audio.cpp's `pocket_tts` model family, via a resident `audiocpp_server` session (see [Overview](#overview)).

audio.cpp's PocketTTS GGUF package ships precomputed voice embeddings for a curated set of named voices, so a built-in voice is selected with `voice: "<id>"` and needs no live cloning step. A custom `.wav` clip dropped into the shared voice folder is still cloned live via `voice_ref`.

**Prerequisites:** none — the GGUF model and voice embeddings download from the (ungated) `audio-cpp/audio.cpp-gguf` mirror with no HuggingFace token required.

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
[VoxCPM2](https://huggingface.co/openbmb/VoxCPM2) is an open-source speech generation model by OpenBMB released under the **Apache-2.0 License**. VoxCtrl runs it through audio.cpp's `voxcpm2` model family, generating rich 24 kHz mono audio.

**Key Features:**
- **Voice Design**: Generate speech using a natural-language description of the speaker voice (`speaker_prompt`), e.g. *"A calm young female voice speaking clearly with a gentle tone."* — passed through as `--instruct <prompt>`.
  > **Known limitation (audio.cpp v0.8.0):** `--instruct` is accepted for `voxcpm2`'s `tts` task but currently has no effect — synthesis always uses the model's default voice regardless of the prompt (confirmed: identical output for contradictory prompts, with or without `--instruct` at all). This is an upstream audio.cpp gap, not a VoxCtrl setting; use Voice Cloning for a specific voice until it's fixed. Breeze-TTS-2's Voice Design has a similar problem (see above) — Voice Cloning is the only mode either engine exposes in the UI.
- **Voice Cloning**: Clone a voice using reference `.wav` audio clips stored in the shared voices directory (`~/.local/share/voxctrl/cloned-tts-voices/`).
- **Ultimate Cloning**: When a paired reference audio and matching transcript file exist in the same directory, the transcript is forwarded as a `reference_text` load option for higher-fidelity cloned synthesis.
- **Model download**: The GGUF package is hosted on the (ungated) `audio-cpp/audio.cpp-gguf` mirror — no HuggingFace token required. Assets are stored in `~/.local/share/voxctrl/models/voxcpm2/` (configurable via `tts.vox_cpm_2.model_dir`).
- **GPU Acceleration**: Optional Vulkan acceleration (`tts.vox_cpm_2.gpu`) with seamless CPU fallback.

### Espeak-ng (Lightweight)
If Piper is unavailable or no voice is downloaded, VoxCtrl can use `espeak-ng`. It is invoked as a subprocess with the text as an argument. Quality is lower but espeak-ng is always available as a system package.

---

## GPU Acceleration

Four engines can run on the GPU, each through its own mechanism:

*   **Piper** (`tts.gpu`): appends the `--cuda` CLI flag to the spawned `piper`
    subprocess at runtime. Needs the app built with the `cuda` feature.
*   **Pocket-TTS** (`tts.pocket_tts.gpu`), **Breeze-TTS-2**
    (`tts.breeze_tts_2.gpu`), and **VoxCPM2** (`tts.vox_cpm_2.gpu`): each pass
    `--backend vulkan` to the `audiocpp_cli` subprocess instead of `--backend
    cpu`. No special build feature is needed — audio.cpp's prebuilt Linux
    release already ships a Vulkan backend alongside CPU.

If a Vulkan device cannot be opened (no compatible GPU, missing driver, or a
build without a Vulkan-capable release), `audiocpp_cli` reports the failure as
an ordinary command error rather than silently falling back — VoxCtrl surfaces
it as a TTS error rather than downgrading automatically. Turn the setting back
off to run on the CPU.

Inflect-Micro-v2 runs on ONNX Runtime's CPU provider. Inflect is small enough
(9.4M parameters) that CPU synthesis is fast; the upstream export also supports
CUDA and DirectML providers, which VoxCtrl does not currently select.

### Requirements & Setup:
1.  A Vulkan-capable GPU and drivers, for the three audio.cpp-backed engines
    (or a CUDA-compatible NVIDIA GPU, for Piper's separate `cuda` feature).
2.  Build with the feature for the engine you want:
    ```bash
    cargo tauri dev --features cuda           # Piper
    ```
    Pocket-TTS, Breeze-TTS-2, and VoxCPM2 need no VoxCtrl build feature —
    Vulkan support lives entirely in the downloaded `audiocpp_cli` binary.

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

VoxCtrl bundles a small catalogue of named voices, each backed by a precomputed embedding shipped in audio.cpp's PocketTTS GGUF package. Selecting a built-in voice passes `--voice-id <id>` straight to `audiocpp_cli` — no live cloning step.

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

Drop a `.wav` reference clip into the configured `pocket_tts.voice_dir` (default `~/.local/share/voxctrl/cloned-tts-voices/`) to add it to the voice list — no re-encoding or extra metadata needed:

- The filename (without extension) becomes the voice's id, e.g. `narrator.wav` adds a voice listed as "Narrator (Custom)".
- Naming a clip after a built-in voice (e.g. `alba.wav`) overrides that voice's bundled embedding, cloning from the clip instead (`--voice-ref`).
- Custom clips are read directly from disk and passed to `audiocpp_cli` as-is; they don't require `download_pocket_tts`.

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

Pocket-TTS downloads the audio.cpp runtime binary (if not already installed), the GGUF model, and the selected voice's embedding — all from the ungated `audio-cpp/audio.cpp-gguf` mirror.

```typescript
// Check if the model and the selected voice's assets are on disk
const ready = await invoke<boolean>('check_pocket_tts_ready', {
  voice: 'alba',
  voiceDir: '',           // '' = default custom-voice directory
});

// Download the audio.cpp runtime (if missing), the model, and the voice embedding.
// hfToken is accepted for parity with the other engines but not required.
// No-op for custom voices resolved from voiceDir — they're already on disk.
await invoke('download_pocket_tts', {
  voice: 'alba',
  voiceDir: '',
  hfToken: null,
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
- **Pocket-TTS, Breeze-TTS-2, and VoxCPM2** each synthesize the whole utterance in one `POST /v1/audio/speech` request to their resident `audiocpp_server` session; the WAV bytes come back in the response body and VoxCtrl decodes them in memory with `rodio::Decoder` (no temp file). Unlike the earlier in-process Candle implementation, playback cannot be interrupted mid-generation — only once the request completes — the same limitation Piper already has.

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

## Pre-warming

`pocket_tts.prewarm`, `breeze_tts_2.prewarm`, `vox_cpm_2.prewarm`, and
`inflect_micro.prewarm` all do the same thing for their respective engine:
load its model at startup instead of on first use.

```json
"tts": {
  "engine": "breeze_tts_2",
  "breeze_tts_2": { "prewarm": true }
}
```

When `prewarm` is `true`, `TtsEngineWorker::start()` enqueues a silent synthesis immediately after spawning the worker thread. The worker processes this short request (a single space) at startup — for Pocket-TTS, Breeze-TTS-2, and VoxCPM2 this spawns their `audiocpp_server` session and sends it that request, which is what actually forces the (lazily-loaded) GGUF model to load; for Inflect-Micro-v2 it loads the ONNX session directly. Subsequent user-triggered syntheses are faster because the model is already warm. This adds startup latency depending on model size and disk speed — several seconds for Breeze-TTS-2's ~5 GB GGUF.

Pre-warming is ignored when `memory_mode` is `"on_demand"` — the two settings want opposite things, and the memory mode wins.

---

## Model Memory (on-demand loading)

Piper and eSpeak-NG run a subprocess per utterance and hold nothing in
between, so `memory_mode` has nothing to do for them. Every other engine
keeps a resident model between utterances — Inflect-Micro-v2 in-process,
Pocket-TTS/Breeze-TTS-2/VoxCPM2 as a resident `audiocpp_server` session (see
[Overview](#overview)) — and `memory_mode` decides whether that residency
survives being idle:

```json
"tts": {
  "engine": "pocket_tts",
  "memory_mode": "on_demand",
  "idle_unload_secs": 900
}
```

| Mode | Behaviour |
|---|---|
| `"always_loaded"` (default) | The model is loaded on first use (or at startup with `prewarm`) and stays resident for the life of the TTS worker. Fastest, highest memory. Recording against a speech target, or a spoken command to one, starts that first load early, as in `"on_demand"` mode. Switching into this mode with `prewarm` on loads the model right away. |
| `"on_demand"` | The model is loaded when VoxCtrl knows it is needed, kept primed while it keeps being used, and dropped after `idle_unload_secs` (default 900 = 15 minutes) of inactivity. |

In `"on_demand"` mode TTS itself stays enabled the whole time — only the weights come and go:

* **Loading starts as early as possible.** `TtsCommand::Preload` is sent the moment a recording
  starts against a target that ends in speech (a `speak` target, or one with a `response_pipe`),
  so the model loads while the user is still talking rather than after they stop. Dictating into
  an ordinary injection target does not load it.
* **The idle countdown restarts on every use.** Speaking an utterance or pre-loading stamps the
  worker's `last_used`, so a back-and-forth conversation never reloads mid-flow. A long utterance
  does not count against the window either — the clock restarts when playback ends.
* **Unloading drops the model** — for Inflect-Micro-v2 that means the in-process ONNX session; for
  Pocket-TTS/Breeze-TTS-2/VoxCPM2 it means killing the resident `audiocpp_server` child process
  outright, not just asking it to free weights, so nothing about the model stays resident. The
  worker thread, its audio device, and the utterance queue stay up, so nothing else about TTS
  changes.
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
| `gpu` | bool | `false` | Enable GPU acceleration (CUDA) for Piper. Pocket-TTS, Breeze-TTS-2, and VoxCPM2 have their own Vulkan GPU settings |
| `hf_token` | string or null | `null` | An optional HuggingFace access token. Not required by any of the audio.cpp-backed engines today (their GGUF mirror is ungated) — kept for parity in case a future model needs it. An exported `HF_TOKEN` takes precedence and is never saved here |
| `pocket_tts.voice` | string | `"alba"` | Default Pocket-TTS voice ID: `"alba"`, `"anna"`, `"vera"`, `"charles"`, `"michael"` |
| `pocket_tts.prewarm` | bool | `false` | Pre-warm the model on startup for faster first synthesis |
| `pocket_tts.voice_dir` | string | `""` | Directory scanned for custom `.wav` voice clips; empty = `~/.local/share/voxctrl/cloned-tts-voices/` |
| `pocket_tts.gpu` | bool | `false` | Enable Vulkan GPU acceleration |
| `breeze_tts_2.voice_mode` | string | `"prompt"` | `"prompt"` for Voice Design, `"clone"` to use a reference clip |
| `breeze_tts_2.speaker_prompt` | string | *(a calm, clear female voice)* | Natural-language description of the speaker, used in `"prompt"` mode |
| `breeze_tts_2.cloned_voice` | string | `"alba"` | Voice id from the shared clip folder, used in `"clone"` mode |
| `breeze_tts_2.voice_dir` | string | `""` | Shared with Pocket-TTS; empty = `~/.local/share/voxctrl/cloned-tts-voices/` |
| `breeze_tts_2.model_dir` | string | `""` | GGUF model directory; empty = `~/.local/share/voxctrl/models/breeze-tts-2/` |
| `breeze_tts_2.prewarm` | bool | `false` | Pre-warm the model on startup for faster first synthesis |
| `breeze_tts_2.gpu` | bool | `false` | Enable Vulkan GPU acceleration |
| `vox_cpm_2.voice_mode` | string | `"prompt"` | `"prompt"` for Voice Design, `"clone"` for reference voice clip |
| `vox_cpm_2.speaker_prompt` | string | *(calm young female voice)* | Natural-language voice description for Voice Design |
| `vox_cpm_2.cloned_voice` | string | `"alba"` | Voice ID from the shared voice clip folder for cloning |
| `vox_cpm_2.voice_dir` | string | `""` | Directory scanned for custom reference clips; empty = platform default |
| `vox_cpm_2.ultimate_cloning` | bool | `false` | Enable Ultimate Cloning when paired audio + transcript files are provided |
| `vox_cpm_2.model_dir` | string | `""` | Directory holding the GGUF model; empty = `~/.local/share/voxctrl/models/voxcpm2/` |
| `vox_cpm_2.prewarm` | bool | `false` | Pre-warm the model on startup for faster first synthesis |
| `vox_cpm_2.gpu` | bool | `false` | Enable Vulkan GPU acceleration |
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
  "pocket_tts": {
    "voice": "alba",
    "voice_dir": "",
    "gpu": true
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

## Pocket-TTS / Breeze-TTS-2 / VoxCPM2 Architecture

All three audio.cpp-backed engines share one session type (`AudioCppSession`
in `crates/voxctrl-tts/src/audiocpp.rs`), differing only in their model
family name and how their speaker is resolved (a `voice`/`voice_ref`, or a
Voice Design prompt — `instruct` for `voxcpm2`, `request_options.instruction`
for `breeze_tts` specifically; see the per-engine notes above).

```
User speaks → transcription → speak_text IPC
                                    │
                         TtsEngineHandle::speak_utterance()
                                    │
                         (bounded channel, cap 32)
                                    │
              speak_pocket_tts() / speak_breeze_tts_2() / speak_vox_cpm_2()
                                    │
              ┌─────────────────────┴──────────────────────┐
              │                                             │
     resolve speaker reference                    resolve_hf_reference_blocking()
     (built-in voice id, a local                   downloads a reference clip from
     voice_ref clip, or an                         the `audio-cpp/audio.cpp-gguf`
     instruct Voice Design prompt)                 mirror into VoxCtrl's own cache
              │                                             │
              └────────────────────┬────────────────────────┘
                                    │
              AudioCppSession::ensure(): spawns `audiocpp_server`
              once per (family, model_dir, gpu) — reused across
              utterances until the config changes or idle-unload
              drops it — writing a one-shot JSON config to a temp
              file (host/port/backend, one lazy-loaded model entry)
                                    │
                    poll GET /health until the server answers
                                    │
              session.speak(): POST /v1/audio/speech
              { "model": <family>, "input": <text>,
                "voice" | "voice_ref" | "instruct" | "request_options",
                "reference_text"? }
              — the model itself only loads on this first request
              (lazy_load), not at server spawn time
                                    │
                 response body is the synthesized WAV, in memory
                                    │
              audiocpp::play_wav_bytes(): rodio::Decoder(Cursor)
                                    │
                           Sink::sleep_until_end()
```

Dropping the `Option<AudioCppSession>` slot (idle-unload, engine switch, or a
changed GPU/model-dir setting) kills the child `audiocpp_server` process via
its `Drop` impl — the model's memory, and the lightweight server process
itself, are both freed. See [Model Memory](#model-memory-on-demand-loading).
