use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    StreamConfig,
};
use crossbeam_channel::{Receiver, Sender};
use tracing::{info, warn};
use voxctrl_config::AudioConfig;

mod denoise;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// Below this peak amplitude, a buffer is treated as silence when deciding
/// whether the microphone is actually delivering signal.
///
/// Windows can deny microphone access *silently*: `build_input_stream` and
/// `Stream::play()` both succeed, but every buffer that arrives is exact
/// zeros. A tiny epsilon rather than a literal zero comparison catches that
/// case (and any hardware that dithers in a few ULPs of noise) while staying
/// far below the self-noise of a real microphone in a quiet room, so genuine
/// quiet speech is never mistaken for a dead stream.
const SILENCE_PEAK_EPSILON: f32 = 1e-4;

/// Whether every sample in `data` falls within the noise floor a denied or
/// dead input stream delivers.
fn is_silent(data: &[f32]) -> bool {
    data.iter().all(|&s| s.abs() < SILENCE_PEAK_EPSILON)
}

/// How much of the audio from just *before* a recording started is kept and
/// prepended to it.
///
/// People start talking as they press the shortcut, not after it, and the
/// opening syllable is the one that carries the wake word. 300 ms covers a
/// syllable or two at a cost of one buffer of memory — 57 KB at 48 kHz — and
/// is short enough that the leading silence it usually contains cannot pull a
/// quiet utterance under the noise gate.
pub const PREROLL_MS: u32 = 300;

/// A chunk of mono f32 audio at TARGET_SAMPLE_RATE Hz.
pub type AudioChunk = Vec<f32>;

pub struct AudioRecorder {
    config: AudioConfig,
    /// Currently recording (pushed to inference queue)
    recording: Arc<AtomicBool>,
    /// Currently monitoring (active settings tab VU meter level feed)
    monitoring: Arc<AtomicBool>,
    /// Live sync dynamic stream preference
    dynamic_stream: Arc<AtomicBool>,
    /// Live input device index, mapped to u32::MAX when None (default system device)
    input_device_index: Arc<AtomicU32>,
    /// Live gain value, stored as f32 bits
    gain: Arc<AtomicU32>,
    /// Live noise-suppression preference
    noise_suppression: Arc<AtomicBool>,
    /// Signalled when a flag this loop watches changes, so it reacts at once
    /// instead of at the next poll. See [`AudioRecorder::with_wake`].
    wake: Option<Receiver<()>>,
}

impl AudioRecorder {
    pub fn new(
        config: AudioConfig,
        recording: Arc<AtomicBool>,
        monitoring: Arc<AtomicBool>,
        dynamic_stream: Arc<AtomicBool>,
        input_device_index: Arc<AtomicU32>,
        gain: Arc<AtomicU32>,
        noise_suppression: Arc<AtomicBool>,
    ) -> Self {
        Self {
            config,
            recording,
            monitoring,
            dynamic_stream,
            input_device_index,
            gain,
            noise_suppression,
            wake: None,
        }
    }

    /// Wake the capture supervisor the moment a flag changes, rather than
    /// leaving it to notice on its next poll.
    ///
    /// In dynamic-stream mode the microphone is opened when recording starts,
    /// so whatever the supervisor spends noticing is dead air at the front of
    /// the utterance — the exact place the wake word lives. With a signal to
    /// wait on, that cost goes to zero and the poll interval underneath can be
    /// long, since it is only a backstop for changes nobody is timing.
    ///
    /// Optional: without it the loop polls as before.
    pub fn with_wake(mut self, wake: Receiver<()>) -> Self {
        self.wake = Some(wake);
        self
    }

    pub fn start_recording(&self) {
        self.recording.store(true, Ordering::SeqCst);
    }

    pub fn stop_recording(&self) {
        self.recording.store(false, Ordering::SeqCst);
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    /// Spawn the audio capture task. Returns a handle to stop it.
    ///
    /// Audio chunks are sent on `tx` only while `recording` is true.
    /// The RMS level (0.0–1.0) is sent on `level_tx` continuously for VU meter.
    pub fn run(
        self,
        tx: Sender<AudioChunk>,
        level_tx: Option<Sender<f32>>,
        audio_ready: Option<Arc<AtomicBool>>,
    ) -> Result<RecorderHandle> {
        let recording = self.recording.clone();
        let monitoring = self.monitoring.clone();
        let dynamic_stream = self.dynamic_stream.clone();
        let input_device_index = self.input_device_index.clone();
        let gain = self.gain.clone();
        let noise_suppression = self.noise_suppression.clone();
        let wake = self.wake.clone();
        let cfg = self.config.clone();

        let handle = std::thread::Builder::new()
            .name("voxctrl-audio".into())
            .spawn(move || {
                if let Err(e) = capture_loop(cfg, gain, noise_suppression, recording, monitoring, dynamic_stream, input_device_index, audio_ready, tx, level_tx, wake) {
                    warn!("Audio capture error: {e}");
                }
            })
            .context("spawn audio thread")?;

        Ok(RecorderHandle { _thread: handle })
    }
}

pub struct RecorderHandle {
    _thread: std::thread::JoinHandle<()>,
}

// ── Device testing at startup ──────────────────────────────────────────────────

pub fn test_and_detect_active_device(idx_opt: Option<u32>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    
    // 1. Try the configured input device if it exists
    if let Some(idx) = idx_opt {
        if let Ok(mut devices) = host.input_devices() {
            if let Some(device) = devices.nth(idx as usize) {
                if let Ok(config) = negotiate_config(&device) {
                    if build_and_start_test_stream(&device, &config) {
                        info!("Startup test: Configured device index {} ({}) is active and functional.", idx, device.name().unwrap_or_default());
                        return Ok(device);
                    } else {
                        warn!("Startup test: Configured device index {} failed test stream build.", idx);
                    }
                }
            }
        }
    }

    // 2. Try default input device
    if let Some(device) = host.default_input_device() {
        if let Ok(config) = negotiate_config(&device) {
            if build_and_start_test_stream(&device, &config) {
                info!("Startup test: Default input device ({}) is active and functional.", device.name().unwrap_or_default());
                return Ok(device);
            } else {
                warn!("Startup test: Default input device failed test stream build.");
            }
        }
    }

    // 3. Fallback: search all available input devices for the first functional one
    if let Ok(devices) = host.input_devices() {
        for (idx, device) in devices.enumerate() {
            if let Ok(config) = negotiate_config(&device) {
                if build_and_start_test_stream(&device, &config) {
                    info!("Startup test: Fallback device index {} ({}) is active and functional.", idx, device.name().unwrap_or_default());
                    return Ok(device);
                }
            }
        }
    }

    // 4. Default fallback
    host.default_input_device().context("no active or functional input device found at startup")
}

/// Build a throwaway input stream on `device` and briefly start it, then stop
/// it — `true` if both succeeded.
///
/// A device can accept `build_input_stream` and still refuse to actually
/// capture: some backends only surface a real failure from `Stream::play()`,
/// not from building the stream. Validating with the same open-then-play
/// sequence the real capture loop uses (see [`open_stream`]) catches that
/// instead of reporting a device "functional" that never actually started.
fn build_and_start_test_stream(device: &cpal::Device, config: &StreamConfig) -> bool {
    let Ok(stream) = device.build_input_stream(config, |_: &[f32], _| {}, |_| {}, None) else {
        return false;
    };
    if stream.play().is_err() {
        return false;
    }
    let _ = stream.pause();
    true
}

// ── Capture processing ────────────────────────────────────────────────────────

/// What one captured buffer produced.
#[derive(Debug)]
struct Captured {
    /// RMS for the VU meter and the overlay, when either is watching.
    level: Option<f32>,
    /// Audio for inference, when recording.
    chunk: Option<AudioChunk>,
}

/// The per-stream processing chain: gain, pre-roll, noise suppression and
/// resampling, and the state each of those carries between buffers.
///
/// Separate from the cpal callback so it can be driven directly by tests —
/// `cpal::InputCallbackInfo` cannot be constructed outside the crate that
/// defines it, which would otherwise put all of this out of reach.
struct CaptureProcessor {
    gain: Arc<AtomicU32>,
    recording: Arc<AtomicBool>,
    monitoring: Arc<AtomicBool>,
    noise_suppression: Arc<AtomicBool>,
    hw_rate: u32,
    /// Interleaved channels per frame the device delivers. Everything after
    /// the downmix works on mono.
    channels: usize,
    /// Reused mono buffer for devices that only capture in stereo (or more),
    /// so the downmix never allocates on the audio thread.
    mono: Vec<f32>,
    needs_resample: bool,
    /// Built on the first buffer that actually needs it, so a stream that runs
    /// with noise suppression off never pays for the model, and a mid-session
    /// toggle still takes effect without rebuilding the stream.
    denoiser: Option<denoise::Denoiser>,
    denoising: bool,
    /// Reused across buffers so the gain stage never allocates on the audio
    /// thread: a `malloc` here is both CPU the real-time deadline cannot spare
    /// and a lock the allocator may block on, which is what an xrun sounds like.
    gained: Vec<f32>,
    /// The tail of what the microphone heard before recording began, at the
    /// hardware rate and without gain applied — gain is whatever it is when the
    /// recording actually starts. Allocated once and only written through, so
    /// idling costs a memcpy per buffer and nothing else.
    preroll: VecDeque<f32>,
    preroll_cap: usize,
    was_recording: bool,
    /// Set once this stream has delivered a buffer that is not silence.
    ///
    /// Windows can deny microphone access without ever surfacing an error:
    /// `build_input_stream` and `play()` both succeed, but the stream then
    /// feeds nothing but exact zeros. Flipping "ready" the moment the stream
    /// opens — which is all `open_stream` on its own can tell — would hide
    /// exactly that case from the watchdog in `pipeline.rs`, so readiness
    /// instead waits for proof that real audio is arriving. It is a one-way
    /// latch for the life of the stream: once the mic has been heard from, a
    /// quiet moment (the user pausing) must not un-ready it.
    audio_ready: Option<Arc<AtomicBool>>,
}

impl CaptureProcessor {
    #[allow(clippy::too_many_arguments)]
    fn new(
        gain: Arc<AtomicU32>,
        recording: Arc<AtomicBool>,
        monitoring: Arc<AtomicBool>,
        noise_suppression: Arc<AtomicBool>,
        hw_rate: u32,
        channels: u16,
        needs_resample: bool,
        audio_ready: Option<Arc<AtomicBool>>,
    ) -> Self {
        let preroll_cap = (hw_rate as usize * PREROLL_MS as usize) / 1000;
        Self {
            gain,
            recording,
            monitoring,
            noise_suppression,
            hw_rate,
            channels: channels.max(1) as usize,
            mono: Vec::new(),
            needs_resample,
            denoiser: None,
            denoising: false,
            gained: Vec::new(),
            preroll: VecDeque::with_capacity(preroll_cap + 1),
            preroll_cap,
            was_recording: false,
            audio_ready,
        }
    }

    /// Process one buffer of interleaved device samples.
    fn feed(&mut self, data: &[f32]) -> Captured {
        if self.channels == 1 {
            return self.feed_mono(data);
        }
        // Taken out and put back so the mono view can be borrowed while the
        // rest of the chain mutates `self`.
        let mut mono = std::mem::take(&mut self.mono);
        downmix_into(data, self.channels, &mut mono);
        let out = self.feed_mono(&mono);
        self.mono = mono;
        out
    }

    fn feed_mono(&mut self, data: &[f32]) -> Captured {
        // Relaxed is enough for every flag read here: each one is an
        // independent switch whose exact flip instant does not order any other
        // memory, and a buffer either side of the change is equally correct.
        let is_recording = self.recording.load(Ordering::Relaxed);
        let current_gain = f32::from_bits(self.gain.load(Ordering::Relaxed));

        // Latch readiness the first time this stream proves it is actually
        // capturing, whether or not a recording is in progress — an always-on
        // or monitoring-only stream is just as good a witness as a recording
        // one, and waiting for a recording to start would only delay the
        // watchdog that depends on this.
        if let Some(ref ready) = self.audio_ready {
            if !ready.load(Ordering::Relaxed) && !is_silent(data) {
                ready.store(true, Ordering::Relaxed);
            }
        }

        // The level feed only has consumers while the overlay is visualising a
        // recording or the Audio tab is showing its VU meter. An always-on
        // stream would otherwise push a level — and with it a Tauri event and
        // a message to the overlay process — for every buffer, all day, with
        // nothing on the other end to draw it.
        //
        // Gain is a scalar, so scaling the RMS is the same number as the RMS
        // of the scaled samples — without touching the heap.
        let level = (is_recording || self.monitoring.load(Ordering::Relaxed))
            .then(|| rms(data) * current_gain);

        if !is_recording {
            self.was_recording = false;
            // Only reachable while the stream is open — an always-on stream, or
            // a dynamic one still up because the Audio tab is monitoring. A
            // closed device hears nothing, so a dynamic recording opened from
            // cold still starts at the moment the device does.
            self.preroll.extend(data.iter().copied());
            let excess = self.preroll.len().saturating_sub(self.preroll_cap);
            self.preroll.drain(..excess);
            return Captured { level, chunk: None };
        }

        // Drop the denoiser when the setting goes off so its next run starts
        // from a clean spectral estimate rather than one built minutes ago.
        let wants_denoise = self.noise_suppression.load(Ordering::Relaxed);
        if wants_denoise != self.denoising {
            self.denoiser = denoise::make_denoiser(wants_denoise, self.hw_rate);
            self.denoising = wants_denoise;
        }

        // The first buffer of a recording carries the pre-roll ahead of it, so
        // the utterance starts where the speaker did rather than where the
        // shortcut landed. Draining leaves the ring empty for the rest of the
        // recording, so this costs nothing after the opening buffer.
        let opening = !self.was_recording;
        self.was_recording = true;
        let carries_preroll = opening && !self.preroll.is_empty();

        // The denoise and resample stages need a slice to borrow, and so does
        // a buffer with the pre-roll in front of it. The straight-through case
        // needs neither: it builds the one buffer handed downstream, no more.
        let processed = if self.denoiser.is_some() || self.needs_resample || carries_preroll {
            self.gained.clear();
            self.gained.reserve(self.preroll.len() + data.len());
            self.gained
                .extend(self.preroll.drain(..).map(|s| s * current_gain));
            self.gained
                .extend(data.iter().map(|&s| s * current_gain));
            match self.denoiser {
                Some(ref mut d) => d.process(&self.gained),
                None if self.needs_resample => {
                    resample_chunk(&self.gained, self.hw_rate, TARGET_SAMPLE_RATE)
                }
                // Once per recording at most: every later buffer takes a
                // branch above or the straight-through one below.
                None => self.gained.clone(),
            }
        } else {
            data.iter().map(|&s| s * current_gain).collect()
        };

        Captured {
            level,
            chunk: (!processed.is_empty()).then_some(processed),
        }
    }
}

// ── Capture callback ──────────────────────────────────────────────────────────

/// Build the data callback cpal invokes for each captured buffer.
///
/// Every stream the capture loop opens — always-on, dynamic, and the one
/// rebuilt after a device change — needs the same processing chain, and a
/// denoiser of its own, so they all come from here rather than being written
/// out three times.
#[allow(clippy::too_many_arguments)]
fn make_input_callback(
    gain: Arc<AtomicU32>,
    recording: Arc<AtomicBool>,
    monitoring: Arc<AtomicBool>,
    level_tx: Option<Sender<f32>>,
    tx: Sender<AudioChunk>,
    noise_suppression: Arc<AtomicBool>,
    hw_rate: u32,
    channels: u16,
    needs_resample: bool,
    audio_ready: Option<Arc<AtomicBool>>,
) -> impl FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static {
    let mut processor = CaptureProcessor::new(
        gain,
        recording,
        monitoring,
        noise_suppression,
        hw_rate,
        channels,
        needs_resample,
        audio_ready,
    );

    move |data: &[f32], _: &cpal::InputCallbackInfo| {
        let out = processor.feed(data);
        if let (Some(ltx), Some(level)) = (level_tx.as_ref(), out.level) {
            let _ = ltx.send(level);
        }
        if let Some(chunk) = out.chunk {
            let _ = tx.send(chunk);
        }
    }
}

// ── Capture loop ──────────────────────────────────────────────────────────────

/// Open and start an input stream on `device` with the standard processing
/// chain, or `None` if the device refused it.
///
/// All three places the capture loop opens a stream — at startup, on the
/// switch to always-on, and when a dynamic recording begins — want exactly
/// this, so they share it rather than repeating the same twenty lines.
#[allow(clippy::too_many_arguments)]
fn open_stream(
    device: &cpal::Device,
    hw_config: &StreamConfig,
    hw_rate: u32,
    needs_resample: bool,
    gain: &Arc<AtomicU32>,
    recording: &Arc<AtomicBool>,
    monitoring: &Arc<AtomicBool>,
    noise_suppression: &Arc<AtomicBool>,
    tx: &Sender<AudioChunk>,
    level_tx: &Option<Sender<f32>>,
    audio_ready: &Option<Arc<AtomicBool>>,
) -> Option<cpal::Stream> {
    let stream = device
        .build_input_stream(
            hw_config,
            make_input_callback(
                gain.clone(),
                recording.clone(),
                monitoring.clone(),
                level_tx.clone(),
                tx.clone(),
                noise_suppression.clone(),
                hw_rate,
                hw_config.channels,
                needs_resample,
                audio_ready.clone(),
            ),
            |e| warn!("Audio stream error: {e}"),
            None,
        )
        .map_err(|e| warn!("Failed to build audio stream: {e}"))
        .ok()?;
    stream
        .play()
        .map_err(|e| warn!("Failed to play audio stream: {e}"))
        .ok()?;
    Some(stream)
}

#[allow(unused_assignments, unused_variables)]
fn capture_loop(
    cfg: AudioConfig,
    gain: Arc<AtomicU32>,
    noise_suppression: Arc<AtomicBool>,
    recording: Arc<AtomicBool>,
    monitoring: Arc<AtomicBool>,
    dynamic_stream: Arc<AtomicBool>,
    input_device_index: Arc<AtomicU32>,
    audio_ready: Option<Arc<AtomicBool>>,
    tx: Sender<AudioChunk>,
    level_tx: Option<Sender<f32>>,
    wake: Option<Receiver<()>>,
) -> Result<()> {
    let host = cpal::default_host();

    let mut current_idx = input_device_index.load(Ordering::SeqCst);
    let idx_opt = if current_idx == u32::MAX { None } else { Some(current_idx) };

    // Perform startup test to detect active device
    let mut device = match test_and_detect_active_device(idx_opt) {
        Ok(d) => d,
        Err(e) => {
            warn!("Startup audio device detection failed: {e}. Falling back to default.");
            host.default_input_device().context("no default input device")?
        }
    };

    info!("Using detected active audio device: {}", device.name().unwrap_or_default());

    let mut hw_config = negotiate_config(&device)?;
    let mut hw_rate = hw_config.sample_rate.0;
    info!("Hardware sample rate: {hw_rate} Hz, {} channel(s)", hw_config.channels);

    let mut needs_resample = hw_rate != TARGET_SAMPLE_RATE;

    let mut current_stream: Option<cpal::Stream> = None;
    let mut was_recording = false;
    let mut was_dynamic = dynamic_stream.load(Ordering::SeqCst);

    // Initial setup based on current preference
    let active_init = recording.load(Ordering::SeqCst) || monitoring.load(Ordering::SeqCst);
    if was_dynamic {
        if active_init {
            // Handled by dynamic loop below
        } else if let Some(ref ready) = audio_ready {
            ready.store(false, Ordering::SeqCst);
        }
    } else {
        info!("Startup: Opening always-on stream (Option B)...");
        if let Some(stream) = open_stream(
            &device, &hw_config, hw_rate, needs_resample,
            &gain, &recording, &monitoring, &noise_suppression, &tx, &level_tx, &audio_ready,
        ) {
            current_stream = Some(stream);
            // Not marked ready here: the stream opening only proves it was
            // *allowed* to open. `audio_ready` is flipped by the capture
            // callback itself, the first time a buffer arrives that isn't
            // silence — see `CaptureProcessor::feed`.
            info!("Startup always-on stream successfully playing.");
        }
    }

    loop {
        let is_recording = recording.load(Ordering::SeqCst);
        let is_monitoring = monitoring.load(Ordering::SeqCst);
        let active = is_recording || is_monitoring;
        let is_dynamic = dynamic_stream.load(Ordering::SeqCst);
        let live_idx = input_device_index.load(Ordering::SeqCst);

        // Detect device change at runtime (live hot-reload!)
        if live_idx != current_idx {
            info!("Device index changed from {current_idx} to {live_idx}, hot-reloading audio device...");
            current_idx = live_idx;
            let idx_opt = if current_idx == u32::MAX { None } else { Some(current_idx) };
            match test_and_detect_active_device(idx_opt) {
                Ok(new_device) => {
                    if let Ok(new_config) = negotiate_config(&new_device) {
                        device = new_device;
                        hw_config = new_config;
                        hw_rate = hw_config.sample_rate.0;
                        needs_resample = hw_rate != TARGET_SAMPLE_RATE;
                        info!("Hot-reload: successfully negotiated new device '{}' ({} Hz)", device.name().unwrap_or_default(), hw_rate);
                        current_stream = None; // Drop old stream
                        was_recording = false; // Force rebuild
                        // A new device has proven nothing yet — carrying the
                        // old device's readiness forward would hide a switch
                        // to a dead or silently-denied one.
                        if let Some(ref ready) = audio_ready {
                            ready.store(false, Ordering::SeqCst);
                        }
                    }
                }
                Err(e) => warn!("Hot-reload failed to find functional device for index {current_idx}: {e}"),
            }
        }

        // 1. Detect dynamic setting change at runtime (live hot-reload!)
        if is_dynamic != was_dynamic {
            info!("Dynamic stream preference changed at runtime to: {is_dynamic}");
            if is_dynamic {
                // Changed to dynamic: turn off always-on stream if not currently active
                if !active {
                    current_stream = None; // Closes device!
                    if let Some(ref ready) = audio_ready {
                        ready.store(false, Ordering::SeqCst);
                    }
                    was_recording = false;
                }
            } else {
                // Changed to always-on: start always-on stream if it isn't already active
                if current_stream.is_none() {
                    if let Some(stream) = open_stream(
                        &device, &hw_config, hw_rate, needs_resample,
                        &gain, &recording, &monitoring, &noise_suppression, &tx, &level_tx, &audio_ready,
                    ) {
                        current_stream = Some(stream);
                        // See the startup open above: readiness is earned by
                        // the callback, not assumed here.
                        info!("Switched to always-on mode: stream successfully playing.");
                    }
                }
            }
            was_dynamic = is_dynamic;
        }

        // 2. Manage dynamic recording stream lifecycles
        if is_dynamic {
            if active && !was_recording {
                info!("Dynamic microphone stream starting (Option A)...");
                match open_stream(
                    &device, &hw_config, hw_rate, needs_resample,
                    &gain, &recording, &monitoring, &noise_suppression, &tx, &level_tx, &audio_ready,
                ) {
                    Some(stream) => {
                        current_stream = Some(stream);
                        // Left false until the callback confirms real audio —
                        // this is exactly the case the Windows silent-denial
                        // bug hits: the stream opens and plays fine, and the
                        // only way to tell is whether any signal ever arrives.
                        info!("Dynamic microphone stream successfully playing (Option A).");
                        was_recording = true;
                    }
                    None if !is_monitoring => recording.store(false, Ordering::SeqCst),
                    None => {}
                }
            } else if !active && was_recording {
                info!("Dynamic microphone stream stopping...");
                if let Some(ref ready) = audio_ready {
                    ready.store(false, Ordering::SeqCst);
                }
                current_stream = None; // Dropping closes the input device!
                was_recording = false;
                info!("Dynamic microphone stream stopped & device closed.");
            }
        }

        // With a wake signal there is nothing left to poll *for* on a short
        // interval: a recording that is about to start says so, and everything
        // else this loop watches is a settings change nobody is timing. The
        // interval is then only a backstop, so it can be long.
        //
        // Without one — an embedder that never called `with_wake` — a dynamic
        // stream still has to notice a recording by looking, and every
        // millisecond it spends looking is missing from the front of the
        // utterance. That case keeps the old 30 ms.
        let backstop = if wake.is_some() || !is_dynamic {
            Duration::from_millis(200)
        } else {
            Duration::from_millis(30)
        };
        match wake {
            Some(ref rx) => match rx.recv_timeout(backstop) {
                Ok(()) => {
                    // Collapse a burst (start then stop, say) into this one
                    // pass; the flags are read fresh at the top of the loop.
                    while rx.try_recv().is_ok() {}
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                // The sender is gone — the app is shutting down around this
                // thread. A disconnected channel returns immediately and
                // forever, so waiting on it here would spin a core through
                // teardown; go back to sleeping out the interval.
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    std::thread::sleep(backstop)
                }
            },
            None => std::thread::sleep(backstop),
        }
    }
}

/// The stream format to open `device` with: its default sample rate, and mono
/// when the device offers mono at that rate — otherwise its own channel count,
/// downmixed in the capture callback.
///
/// Mono used to be requested unconditionally. Plenty of devices only capture
/// in stereo — many USB microphones, and WASAPI's shared-mode mix format is
/// often two channels — and those refuse a mono stream outright, so the
/// microphone never opened.
fn negotiate_config(device: &cpal::Device) -> Result<StreamConfig> {
    let default = device.default_input_config()?;
    let rate = default.sample_rate();
    let mono_supported = device
        .supported_input_configs()
        .map(|mut configs| {
            configs.any(|c| {
                c.channels() == 1 && c.min_sample_rate() <= rate && rate <= c.max_sample_rate()
            })
        })
        .unwrap_or(false);
    Ok(StreamConfig {
        channels: if mono_supported { 1 } else { default.channels() },
        sample_rate: rate,
        buffer_size: cpal::BufferSize::Default,
    })
}

/// Average interleaved `channels`-wide frames of `data` into mono, replacing
/// the contents of `out`. A trailing partial frame is dropped.
fn downmix_into(data: &[f32], channels: usize, out: &mut Vec<f32>) {
    out.clear();
    let scale = 1.0 / channels as f32;
    out.extend(
        data.chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * scale),
    );
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f32 = samples.iter().map(|&s| s * s).sum();
    (sum_sq / samples.len() as f32).sqrt()
}

/// Linear resampling between two rates, appended to `out`.
///
/// The source position walks forward by a fixed step rather than being
/// recomputed as `i / ratio` per output sample, which keeps this to one add,
/// one truncate and one multiply-add per sample — it runs on the audio
/// callback thread for every buffer a non-16 kHz device delivers.
///
/// Writing into a caller-owned buffer lets the denoiser resample twice per
/// chunk without allocating twice per chunk.
pub(crate) fn resample_into(input: &[f32], from_hz: u32, to_hz: u32, out: &mut Vec<f32>) {
    if input.is_empty() {
        return;
    }
    if from_hz == to_hz {
        out.extend_from_slice(input);
        return;
    }
    let ratio = to_hz as f64 / from_hz as f64;
    let out_len = (input.len() as f64 * ratio) as usize;
    let step = 1.0 / ratio;
    let last = input.len() - 1;

    out.reserve(out_len);
    let mut src = 0.0f64;
    for _ in 0..out_len {
        // Clamped because `src` is accumulated rather than recomputed: the
        // drift is far too small to hear, but it must not index past the end.
        let lo = (src as usize).min(last);
        let frac = (src - lo as f64) as f32;
        let hi = (lo + 1).min(last);
        out.push(input[lo] * (1.0 - frac) + input[hi] * frac);
        src += step;
    }
}

fn resample_chunk(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    let mut out = Vec::new();
    resample_into(input, from_hz, to_hz, &mut out);
    out
}

// ── Device listing ────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct AudioDeviceInfo {
    pub index: u32,
    pub name: String,
}

pub fn list_input_devices() -> Vec<AudioDeviceInfo> {
    let host = cpal::default_host();
    host.input_devices()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(i, d)| AudioDeviceInfo {
            index: i as u32,
            name: d.name().unwrap_or_else(|_| format!("Device {i}")),
        })
        .collect()
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    /// A processor at the target rate, so buffer lengths pass through
    /// unchanged and the pre-roll arithmetic is exact.
    fn processor(recording: &Arc<AtomicBool>) -> CaptureProcessor {
        CaptureProcessor::new(
            Arc::new(AtomicU32::new(1.0f32.to_bits())),
            recording.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            TARGET_SAMPLE_RATE,
            1,
            false,
            None,
        )
    }

    fn ramp(n: usize, start: f32) -> Vec<f32> {
        (0..n).map(|i| start + i as f32).collect()
    }

    /// The bug: dictation starts when the shortcut fires, but people are
    /// already talking by then. Whatever the microphone heard just before has
    /// to arrive in front of the recording, or the opening word — the one
    /// carrying the wake word — is simply not in the audio.
    #[test]
    fn the_opening_buffer_carries_what_came_before_it() {
        let recording = Arc::new(AtomicBool::new(false));
        let mut p = processor(&recording);

        p.feed(&ramp(100, 0.0));
        p.feed(&ramp(100, 100.0));
        assert!(p.feed(&[0.0; 10]).chunk.is_none(), "idle audio was recorded");

        recording.store(true, Ordering::SeqCst);
        let first = p.feed(&ramp(50, 1000.0)).chunk.expect("recording produced nothing");

        // 210 buffered samples ahead of the 50 live ones.
        assert_eq!(first.len(), 260, "pre-roll was not prepended");
        assert_eq!(first[0], 0.0, "pre-roll did not start at the oldest sample");
        assert_eq!(first[199], 199.0, "pre-roll was not contiguous");
        assert_eq!(first[210], 1000.0, "live audio did not follow the pre-roll");
    }

    #[test]
    fn the_pre_roll_is_spent_once_and_not_repeated() {
        let recording = Arc::new(AtomicBool::new(false));
        let mut p = processor(&recording);
        p.feed(&ramp(100, 0.0));

        recording.store(true, Ordering::SeqCst);
        assert_eq!(p.feed(&ramp(50, 0.0)).chunk.unwrap().len(), 150);
        assert_eq!(
            p.feed(&ramp(50, 0.0)).chunk.unwrap().len(),
            50,
            "the pre-roll was replayed into a later buffer"
        );
    }

    /// The ring is the only thing here that grows, so it has to be bounded by
    /// something other than how long the app has been idle.
    #[test]
    fn the_pre_roll_never_grows_past_its_window() {
        let recording = Arc::new(AtomicBool::new(false));
        let mut p = processor(&recording);
        let cap = (TARGET_SAMPLE_RATE as usize * PREROLL_MS as usize) / 1000;

        for _ in 0..200 {
            p.feed(&[0.5; 512]);
        }
        assert_eq!(p.preroll.len(), cap, "the ring drifted from its window");

        recording.store(true, Ordering::SeqCst);
        assert_eq!(p.feed(&[0.0; 10]).chunk.unwrap().len(), cap + 10);
    }

    /// A second recording must open with the audio from just before *it*, not
    /// with leftovers from the previous one.
    #[test]
    fn a_later_recording_gets_its_own_pre_roll() {
        let recording = Arc::new(AtomicBool::new(false));
        let mut p = processor(&recording);

        p.feed(&ramp(100, 0.0));
        recording.store(true, Ordering::SeqCst);
        p.feed(&ramp(10, 0.0));
        recording.store(false, Ordering::SeqCst);

        p.feed(&ramp(70, 500.0));
        recording.store(true, Ordering::SeqCst);
        let second = p.feed(&ramp(10, 0.0)).chunk.unwrap();
        assert_eq!(second.len(), 80, "the second recording lost its pre-roll");
        assert_eq!(second[0], 500.0, "stale audio led the second recording");
    }

    #[test]
    fn gain_applies_to_the_pre_roll_as_well_as_the_live_audio() {
        let recording = Arc::new(AtomicBool::new(false));
        let mut p = processor(&recording);
        p.gain.store(2.0f32.to_bits(), Ordering::SeqCst);

        p.feed(&[1.0; 20]);
        recording.store(true, Ordering::SeqCst);
        let chunk = p.feed(&[3.0; 5]).chunk.unwrap();

        assert!(chunk[..20].iter().all(|&s| s == 2.0), "pre-roll missed the gain");
        assert!(chunk[20..].iter().all(|&s| s == 6.0), "live audio missed the gain");
    }

    /// The level feed exists for the VU meter and the overlay; with neither
    /// watching it must stay silent, which is what keeps an idle always-on
    /// stream from pushing a hundred IPC messages a second.
    #[test]
    fn levels_are_produced_only_for_someone_watching() {
        let recording = Arc::new(AtomicBool::new(false));
        let monitoring = Arc::new(AtomicBool::new(false));
        let mut p = CaptureProcessor::new(
            Arc::new(AtomicU32::new(1.0f32.to_bits())),
            recording.clone(),
            monitoring.clone(),
            Arc::new(AtomicBool::new(false)),
            TARGET_SAMPLE_RATE,
            1,
            false,
            None,
        );

        assert!(p.feed(&[0.5; 16]).level.is_none(), "idle stream reported a level");

        monitoring.store(true, Ordering::SeqCst);
        assert_eq!(p.feed(&[0.5; 16]).level, Some(0.5), "the VU meter got nothing");

        monitoring.store(false, Ordering::SeqCst);
        recording.store(true, Ordering::SeqCst);
        assert_eq!(p.feed(&[0.5; 16]).level, Some(0.5), "the overlay got nothing");
    }

    #[test]
    fn silence_classifies_exact_zero_and_near_zero_buffers() {
        assert!(is_silent(&[0.0; 32]), "an all-zero buffer must read as silent");
        assert!(
            is_silent(&[0.0, 1e-6, -1e-6, 0.0]),
            "dither far below the noise floor must still read as silent"
        );
        assert!(
            !is_silent(&[0.0, 0.0, 0.01, 0.0]),
            "a single real sample must be enough to call a buffer non-silent"
        );
    }

    /// The Windows bug this exists for: a stream can open and `play()`
    /// successfully while WASAPI hands it nothing but zeros, with no error
    /// anywhere. `audio_ready` must stay false until a buffer proves
    /// otherwise, and — once it has — must not flap back to false just
    /// because the user paused talking.
    #[test]
    fn audio_ready_latches_on_first_real_signal_and_not_before() {
        let recording = Arc::new(AtomicBool::new(true));
        let ready = Arc::new(AtomicBool::new(false));
        let mut p = CaptureProcessor::new(
            Arc::new(AtomicU32::new(1.0f32.to_bits())),
            recording.clone(),
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicBool::new(false)),
            TARGET_SAMPLE_RATE,
            1,
            false,
            Some(ready.clone()),
        );

        p.feed(&[0.0; 64]);
        assert!(!ready.load(Ordering::SeqCst), "silence must not mark the stream ready");

        p.feed(&[0.02; 64]);
        assert!(ready.load(Ordering::SeqCst), "real signal must mark the stream ready");

        p.feed(&[0.0; 64]);
        assert!(
            ready.load(Ordering::SeqCst),
            "a later quiet buffer must not un-latch a confirmed stream"
        );
    }

    #[test]
    fn stereo_frames_are_averaged_to_mono() {
        let mut out = vec![9.0; 4];
        downmix_into(&[1.0, 3.0, -1.0, 1.0, 0.5], 2, &mut out);
        assert_eq!(out, vec![2.0, 0.0], "frames were not averaged, or the partial frame was kept");
    }

    /// A stereo-only microphone delivers interleaved frames; everything
    /// downstream — pre-roll, gain, the level and the recording — must see
    /// one sample per frame, not twice as many.
    #[test]
    fn a_stereo_stream_records_mono() {
        let recording = Arc::new(AtomicBool::new(false));
        let mut p = CaptureProcessor::new(
            Arc::new(AtomicU32::new(1.0f32.to_bits())),
            recording.clone(),
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(false)),
            TARGET_SAMPLE_RATE,
            2,
            false,
            None,
        );

        let idle = p.feed(&[0.2, 0.4, 0.2, 0.4]);
        assert!((idle.level.unwrap() - 0.3).abs() < 1e-6, "level was not taken from the mono mix");
        assert_eq!(p.preroll.len(), 2, "pre-roll held interleaved samples");

        recording.store(true, Ordering::SeqCst);
        let chunk = p.feed(&[1.0, 0.0, 0.0, 1.0, 0.5, 0.5]).chunk.unwrap();
        assert_eq!(chunk.len(), 2 + 3);
        assert!(chunk[2..].iter().all(|&s| (s - 0.5).abs() < 1e-6));
    }
}
