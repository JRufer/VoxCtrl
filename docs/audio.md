# Audio Pipeline

**Crate:** `crates/voxctr-audio/`

## Responsibilities

- Enumerate and select audio input devices
- Stream raw PCM from the microphone via CPAL
- Resample from the hardware rate to 16 kHz (Whisper's required input rate)
- Compute RMS levels for the VU meter and noise gate
- Support two streaming modes: dynamic (on-demand) and always-on

---

## Device Selection

On startup, `test_and_detect_active_device()` probes devices in priority order:

1. **Configured device** — `audio.device_index` from config
2. **Default input device** — CPAL's `default_input_device()`
3. **First enumerable device** — iterates all hosts/devices

A "test capture" of ~200ms is performed to confirm the device actually produces audio before committing to it. The result is stored atomically so the inference and UI layers can query it.

Devices are listed with `list_audio_devices()` which returns `(index, name)` pairs for the Settings → Audio tab.

---

## Streaming Modes

### Dynamic Streaming (default)
The microphone stream is opened when recording starts and closed when it stops. This is the default mode.

- Lower CPU/battery usage when idle
- A small startup latency (~10–50ms) when a hotkey is pressed
- Suitable for most users

### Always-On Streaming
The stream stays open permanently. Chunks are buffered and discarded when not recording.

- Zero startup latency
- Higher idle CPU usage
- Required for VAD-triggered recording (future feature)

Toggle via `audio.dynamic_stream` in config.

---

## Audio Processing Chain

```
Hardware Input (e.g. 48000 Hz, f32 or i16 samples)
        │
        ▼
   normalize_samples()     — convert i16/i32/u16 to f32 [-1.0, 1.0]
        │
        ▼
   apply_gain()             — multiply by gain factor (default 1.0)
        │
        ▼
   resample_chunk()         — linear interpolation to 16000 Hz
        │
        ▼
   audio_tx.send(chunk)     — Vec<f32> forwarded to lib.rs accumulator
        │
        ▼
   rms() → audio_level_tx   — f32 RMS forwarded to UI for visualization
```

### Resampling

`resample_chunk(input, from_rate, to_rate)` uses linear interpolation via the **Rubato** library. This is fast and introduces minimal latency, which is more important than perfect audio quality for speech recognition.

### Gain Control

`audio.gain` (0.0–4.0) is stored as an `AtomicU32` (bit-cast from f32) so it can be updated from the UI without locking the audio thread.

---

## Noise Gate / VAD

A simple energy-based Voice Activity Detection gate is applied during inference, not capture:

```
RMS energy of captured audio
        │
        ▼
    < audio.vad_threshold?   →  Drop (return empty string)
        │
        ▼
    Send to Whisper
```

The threshold is configurable (default ~0.02). Setting it too high causes missed speech; too low causes Whisper to transcribe silence as hallucinated text (a known Whisper behavior).

---

## Configuration Options

All under `audio` in `config.json`:

| Key | Type | Default | Description |
|---|---|---|---|
| `device_index` | `i32` | `-1` (auto) | CPAL device index; -1 = auto-detect |
| `gain` | `f32` | `1.0` | Microphone gain multiplier (0.0–4.0) |
| `vad_threshold` | `f32` | `0.02` | RMS threshold below which audio is discarded |
| `dynamic_stream` | `bool` | `true` | Open/close mic stream on demand |

---

## AudioRecorder API

```rust
// Create recorder with a channel for audio chunks and RMS levels
let recorder = AudioRecorder::new(audio_tx, level_tx, config);

// Start capturing (dynamic mode: opens stream)
recorder.start()?;

// Stop capturing (dynamic mode: closes stream)
recorder.stop()?;

// Update gain at runtime (atomic, no restart needed)
recorder.set_gain(1.5);

// Switch device at runtime (triggers stream restart)
recorder.set_device_index(2);
```
