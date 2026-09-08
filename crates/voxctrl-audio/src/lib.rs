use std::{
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleRate, StreamConfig,
};
use crossbeam_channel::Sender;
use tracing::{info, warn};
use voxctrl_config::AudioConfig;

mod denoise;
use denoise::make_denoiser;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

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
        }
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
        let cfg = self.config.clone();

        let handle = std::thread::Builder::new()
            .name("voxctrl-audio".into())
            .spawn(move || {
                if let Err(e) = capture_loop(cfg, gain, noise_suppression, recording, monitoring, dynamic_stream, input_device_index, audio_ready, tx, level_tx) {
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
                    let test_stream = device.build_input_stream(
                        &config,
                        |_: &[f32], _| {},
                        |_| {},
                        None,
                    );
                    if test_stream.is_ok() {
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
            let test_stream = device.build_input_stream(
                &config,
                |_: &[f32], _| {},
                |_| {},
                None,
            );
            if test_stream.is_ok() {
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
                let test_stream = device.build_input_stream(
                    &config,
                    |_: &[f32], _| {},
                    |_| {},
                    None,
                );
                if test_stream.is_ok() {
                    info!("Startup test: Fallback device index {} ({}) is active and functional.", idx, device.name().unwrap_or_default());
                    return Ok(device);
                }
            }
        }
    }

    // 4. Default fallback
    host.default_input_device().context("no active or functional input device found at startup")
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
    needs_resample: bool,
) -> impl FnMut(&[f32], &cpal::InputCallbackInfo) + Send + 'static {
    // Built on the first buffer that actually needs it, so a stream that runs
    // with noise suppression off never pays for the model, and a mid-session
    // toggle still takes effect without rebuilding the stream.
    let mut denoiser = None;
    let mut denoising = false;
    // Reused across callbacks so the gain stage never allocates on the audio
    // thread: a `malloc` here is both CPU the real-time deadline cannot spare
    // and a lock the allocator may block on, which is what an xrun sounds like.
    let mut gained: Vec<f32> = Vec::new();

    move |data: &[f32], _: &cpal::InputCallbackInfo| {
        // Relaxed is enough for every flag read here: each one is an
        // independent switch whose exact flip instant does not order any other
        // memory, and a buffer either side of the change is equally correct.
        let is_recording = recording.load(Ordering::Relaxed);

        // The level feed only has consumers while the overlay is visualising a
        // recording or the Audio tab is showing its VU meter. An always-on
        // stream would otherwise push a level — and with it a Tauri event and
        // a message to the overlay process — for every buffer, all day, with
        // nothing on the other end to draw it.
        if let Some(ref ltx) = level_tx {
            if is_recording || monitoring.load(Ordering::Relaxed) {
                // Gain is a scalar, so scaling the RMS is the same number as
                // the RMS of the scaled samples — without touching the heap.
                let level = rms(data) * f32::from_bits(gain.load(Ordering::Relaxed));
                let _ = ltx.send(level);
            }
        }

        if !is_recording {
            return;
        }

        let current_gain = f32::from_bits(gain.load(Ordering::Relaxed));

        // Drop the denoiser when the setting goes off so its next run starts
        // from a clean spectral estimate rather than one built minutes ago.
        let wants_denoise = noise_suppression.load(Ordering::Relaxed);
        if wants_denoise != denoising {
            denoiser = make_denoiser(wants_denoise, hw_rate);
            denoising = wants_denoise;
        }

        // The denoise and resample stages need a slice to borrow, so those two
        // fill the scratch buffer. The straight-through case does not: it
        // builds the one buffer that is handed downstream and nothing more.
        let processed = if let Some(ref mut d) = denoiser {
            gained.clear();
            gained.extend(data.iter().map(|&s| s * current_gain));
            d.process(&gained)
        } else if needs_resample {
            gained.clear();
            gained.extend(data.iter().map(|&s| s * current_gain));
            resample_chunk(&gained, hw_rate, TARGET_SAMPLE_RATE)
        } else {
            data.iter().map(|&s| s * current_gain).collect()
        };
        if processed.is_empty() {
            return;
        }
        let _ = tx.send(processed);
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
                needs_resample,
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
    info!("Hardware sample rate: {hw_rate} Hz");

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
            &gain, &recording, &monitoring, &noise_suppression, &tx, &level_tx,
        ) {
            current_stream = Some(stream);
            if let Some(ref ready) = audio_ready {
                ready.store(true, Ordering::SeqCst);
            }
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
                        &gain, &recording, &monitoring, &noise_suppression, &tx, &level_tx,
                    ) {
                        current_stream = Some(stream);
                        if let Some(ref ready) = audio_ready {
                            ready.store(true, Ordering::SeqCst);
                        }
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
                    &gain, &recording, &monitoring, &noise_suppression, &tx, &level_tx,
                ) {
                    Some(stream) => {
                        current_stream = Some(stream);
                        if let Some(ref ready) = audio_ready {
                            ready.store(true, Ordering::SeqCst);
                        }
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

        // 30 ms is what a dynamic stream needs: it opens the device on the
        // hotkey press, so the poll interval is dead air at the start of a
        // recording. An always-on stream has its device open already and this
        // loop is only watching for a settings change, which nobody notices
        // arriving a fifth of a second later — so it idles at a rate that
        // lets the CPU stay asleep instead of waking 33 times a second.
        std::thread::sleep(if is_dynamic {
            Duration::from_millis(30)
        } else {
            Duration::from_millis(200)
        });
    }
}

fn negotiate_config(device: &cpal::Device) -> Result<StreamConfig> {
    let supported = device.default_input_config()?;
    Ok(StreamConfig {
        channels: 1,
        sample_rate: SampleRate(supported.sample_rate().0),
        buffer_size: cpal::BufferSize::Default,
    })
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
