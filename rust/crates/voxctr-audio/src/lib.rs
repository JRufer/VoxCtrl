use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use anyhow::{Context, Result};
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    SampleFormat, SampleRate, StreamConfig,
};
use crossbeam_channel::Sender;
use rubato::{FftFixedIn, Resampler};
use tracing::{debug, info, warn};
use voxctr_config::AudioConfig;

pub const TARGET_SAMPLE_RATE: u32 = 16_000;

/// A chunk of mono f32 audio at TARGET_SAMPLE_RATE Hz.
pub type AudioChunk = Vec<f32>;

pub struct AudioRecorder {
    config: AudioConfig,
    /// Currently recording (pushed to inference queue)
    recording: Arc<AtomicBool>,
    /// Gain applied to all samples
    gain: f32,
}

impl AudioRecorder {
    pub fn new(config: AudioConfig) -> Self {
        let gain = config.gain;
        Self {
            config,
            recording: Arc::new(AtomicBool::new(false)),
            gain,
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
    ) -> Result<RecorderHandle> {
        let recording = self.recording.clone();
        let cfg = self.config.clone();
        let gain = self.gain;

        let handle = std::thread::Builder::new()
            .name("voxctr-audio".into())
            .spawn(move || {
                if let Err(e) = capture_loop(cfg, gain, recording, tx, level_tx) {
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

// ── Capture loop ──────────────────────────────────────────────────────────────

fn capture_loop(
    cfg: AudioConfig,
    gain: f32,
    recording: Arc<AtomicBool>,
    tx: Sender<AudioChunk>,
    level_tx: Option<Sender<f32>>,
) -> Result<()> {
    let host = cpal::default_host();

    let device = if let Some(idx) = cfg.input_device_index {
        host.input_devices()?
            .nth(idx as usize)
            .context("device index out of range")?
    } else {
        host.default_input_device().context("no default input device")?
    };

    info!("Audio device: {}", device.name().unwrap_or_default());

    let hw_config = negotiate_config(&device)?;
    let hw_rate = hw_config.sample_rate.0;
    info!("Hardware sample rate: {hw_rate} Hz");

    // Buffer for accumulating resampled audio
    let acc = Arc::new(std::sync::Mutex::new(Vec::<f32>::new()));
    let acc_inner = acc.clone();
    let tx_inner = tx.clone();
    let level_tx_inner = level_tx;
    let recording_inner = recording.clone();

    let needs_resample = hw_rate != TARGET_SAMPLE_RATE;
    let resample_ratio = TARGET_SAMPLE_RATE as f64 / hw_rate as f64;

    // Build resampler (runs inside the callback context, sent via closure)
    let stream = device.build_input_stream(
        &hw_config,
        move |data: &[f32], _| {
            // Apply gain
            let gained: Vec<f32> = data.iter().map(|&s| s * gain).collect();

            // RMS level for VU meter
            if let Some(ref ltx) = level_tx_inner {
                let rms = rms(&gained);
                let _ = ltx.send(rms);
            }

            if !recording_inner.load(Ordering::SeqCst) {
                return;
            }

            // Resample to 16 kHz if needed
            let resampled = if needs_resample {
                resample_chunk(&gained, hw_rate, TARGET_SAMPLE_RATE)
            } else {
                gained
            };

            let _ = tx_inner.send(resampled);
        },
        |e| warn!("Audio stream error: {e}"),
        None,
    )?;

    stream.play()?;

    // Keep the thread alive (stream is playing in background callbacks)
    loop {
        std::thread::sleep(Duration::from_millis(100));
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

/// Simple linear resampling. Replaced by rubato for production quality.
fn resample_chunk(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() {
        return input.to_vec();
    }
    let ratio = to_hz as f64 / from_hz as f64;
    let out_len = (input.len() as f64 * ratio) as usize;
    (0..out_len)
        .map(|i| {
            let src_idx = i as f64 / ratio;
            let lo = src_idx as usize;
            let hi = (lo + 1).min(input.len() - 1);
            let frac = src_idx - lo as f64;
            input[lo] * (1.0 - frac as f32) + input[hi] * frac as f32
        })
        .collect()
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
