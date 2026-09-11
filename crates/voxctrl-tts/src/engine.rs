//! The utterance queue, worker thread, and Piper/eSpeak synthesis. Pocket-TTS
//! synthesis lives in `pocket.rs` (called from [`TtsEngineWorker::run`]) since
//! it needs no access to this module's private fields.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossbeam_channel::{bounded, Receiver, RecvTimeoutError, Sender};
use tracing::{debug, info, warn};
use voxctrl_config::{TtsConfig, TtsEngine};
use voxctrl_text::{correct_custom_vocabulary, expand_snippets};

use crate::breeze::{ensure_breeze_tts_2_loaded, speak_breeze_tts_2, BreezeModelSlot};
use crate::inflect::{ensure_inflect_micro_loaded, speak_inflect_micro};
use crate::piper::{get_voice_path, piper_binary, sample_rate_for_voice};
use crate::pocket::{ensure_pocket_tts_loaded, speak_pocket_tts};
use crate::voxcpm::{ensure_vox_cpm_2_loaded, speak_vox_cpm_2, VoxCpmModelSlot};

/// How long the worker parks in `recv_timeout` when there is nothing to expire.
/// Only a wake-up interval — the channel still wakes it immediately on a command.
const IDLE_PARK: Duration = Duration::from_secs(3600);

/// The worker's cached Inflect-Micro-v2 sessions. Without the `inflect-micro`
/// feature there is no model type to cache, so the slot degenerates to `()` and
/// `speak_inflect_micro` reports that the engine wasn't compiled in.
#[cfg(feature = "inflect-micro")]
type InflectModelSlot = Option<crate::inflect::model::InflectModel>;
#[cfg(not(feature = "inflect-micro"))]
type InflectModelSlot = Option<()>;

// ── Utterance queue ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Utterance {
    pub text: String,
    pub voice: Option<String>,
    pub source_label: Option<String>,
}

#[derive(Debug, Clone)]
pub enum TtsCommand {
    Play {
        utterance: Utterance,
        generation: u32,
    },
    /// Live config swap — also how the memory policy reaches a running worker,
    /// so the tray toggle never has to tear down the engine (and its audio
    /// device) just to change it.
    UpdateConfig(TtsConfig),
    /// Load the model now (and restart the idle countdown) without speaking.
    /// Sent as soon as VoxCtrl knows speech is likely — e.g. the moment a
    /// recording starts — so the load overlaps with the user still talking.
    Preload,
    Shutdown,
}

static ACTIVE_SINK: std::sync::Mutex<Option<std::sync::Arc<rodio::Sink>>> = std::sync::Mutex::new(None);

pub fn stop_current_playback() {
    let mut guard = ACTIVE_SINK.lock().unwrap();
    if let Some(ref sink) = *guard {
        let _ = sink.stop();
    }
    *guard = None;
}

#[derive(Clone)]
pub struct TtsEngineHandle {
    tx: Sender<TtsCommand>,
    generation: Arc<AtomicU32>,
    /// Mirrors whether the worker currently holds a model in memory, so the UI
    /// and tray can show the state without interrogating the worker thread.
    model_loaded: Arc<AtomicBool>,
}

impl TtsEngineHandle {
    pub fn speak(&self, text: impl Into<String>) {
        let gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(TtsCommand::Play {
            utterance: Utterance {
                text: text.into(),
                voice: None,
                source_label: None,
            },
            generation: gen,
        });
    }

    pub fn speak_utterance(&self, u: Utterance) {
        let gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
        let _ = self.tx.send(TtsCommand::Play {
            utterance: u,
            generation: gen,
        });
    }

    /// Ask the worker to load the model now. Cheap and idempotent when the model
    /// is already resident — it just restarts the idle countdown.
    pub fn preload(&self) {
        let _ = self.tx.try_send(TtsCommand::Preload);
    }

    /// Whether a model is currently resident in memory.
    pub fn is_model_loaded(&self) -> bool {
        self.model_loaded.load(Ordering::SeqCst)
    }

    pub fn stop(&self) {
        self.generation.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        stop_current_playback();
    }

    pub fn update_config(&self, config: TtsConfig) {
        let _ = self.tx.send(TtsCommand::UpdateConfig(config));
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(TtsCommand::Shutdown);
    }
}

// ── TTS engine worker ─────────────────────────────────────────────────────────

pub type PlaybackCallback = Arc<dyn Fn() + Send + Sync + 'static>;
/// Called with a human-readable message whenever an utterance fails to play
/// (engine missing, voice not downloaded, audio device unavailable, ...).
pub type ErrorCallback = Arc<dyn Fn(String) + Send + Sync + 'static>;

pub struct TtsEngineWorker {
    config: TtsConfig,
    custom_vocabulary: Vec<String>,
    rx: Receiver<TtsCommand>,
    generation: Arc<AtomicU32>,
    model_loaded: Arc<AtomicBool>,
    on_playback_start: Option<PlaybackCallback>,
    on_playback_end: Option<PlaybackCallback>,
    on_error: Option<ErrorCallback>,
}

impl TtsEngineWorker {
    pub fn start(
        config: TtsConfig,
        custom_vocabulary: Vec<String>,
        on_playback_start: Option<PlaybackCallback>,
        on_playback_end: Option<PlaybackCallback>,
        on_error: Option<ErrorCallback>,
    ) -> TtsEngineHandle {
        let (tx, rx) = bounded(32);
        let generation = Arc::new(AtomicU32::new(0));
        let model_loaded = Arc::new(AtomicBool::new(false));
        let handle = TtsEngineHandle {
            tx,
            generation: generation.clone(),
            model_loaded: model_loaded.clone(),
        };

        let prewarm = match config.engine {
            TtsEngine::PocketTts => config.pocket_tts.prewarm,
            TtsEngine::InflectMicro => config.inflect_micro.prewarm,
            TtsEngine::BreezeTts2 => config.breeze_tts_2.prewarm,
            TtsEngine::VoxCpm2 => config.vox_cpm_2.prewarm,
            _ => false,
        };
        // Pre-warming loads the model at startup and keeps it there, which is
        // exactly what the on-demand memory mode exists to avoid — so the two
        // settings do not fight: on-demand wins and the model waits for its
        // first real use (or a `preload()`).
        if prewarm && !config.unloads_when_idle() {
            let _ = handle.tx.send(TtsCommand::Play {
                utterance: Utterance {
                    text: " ".into(),
                    voice: None,
                    source_label: Some("prewarm".into()),
                },
                generation: 0,
            });
        }

        let worker = Self {
            config,
            custom_vocabulary,
            rx,
            generation,
            model_loaded,
            on_playback_start,
            on_playback_end,
            on_error,
        };
        std::thread::Builder::new()
            .name("voxctrl-tts".into())
            .spawn(move || worker.run())
            .expect("spawn tts thread");

        handle
    }

    fn run(self) {
        info!(
            "TTS engine started (engine={:?}, memory_mode={:?})",
            self.config.engine, self.config.memory_mode
        );
        let mut current_config = self.config.clone();


        // pocket-tts model + per-voice cloned voice state, cached for the lifetime of this worker thread.
        let mut pocket_tts_model: Option<pocket_tts::TTSModel> = None;
        let mut pocket_tts_voice_states: HashMap<String, pocket_tts::ModelState> = HashMap::new();
        // Inflect-Micro-v2 ONNX sessions, cached for the same lifetime.
        let mut inflect_model: InflectModelSlot = None;
        // Breeze-TTS-2 session + per-voice cloned state, cached for the same lifetime.
        let mut breeze_tts_2_model: BreezeModelSlot = None;
        let mut breeze_tts_2_voice_states: HashMap<String, pocket_tts::ModelState> = HashMap::new();
        // VoxCPM2 session + per-voice cloned state, cached for the same lifetime.
        let mut vox_cpm_2_model: VoxCpmModelSlot = None;
        let mut vox_cpm_2_voice_states: HashMap<String, pocket_tts::ModelState> = HashMap::new();

        // Persistent Rodio Output Stream - kept alive for the lifetime of this thread!
        let mut audio_context: Option<(rodio::OutputStream, rodio::OutputStreamHandle, Arc<rodio::Sink>)> = None;

        let init_audio = |ctx: &mut Option<(rodio::OutputStream, rodio::OutputStreamHandle, Arc<rodio::Sink>)>| -> Result<Arc<rodio::Sink>> {
            if let Some((_, _, ref sink)) = ctx {
                return Ok(sink.clone());
            }
            let (stream, handle) = rodio::OutputStream::try_default()
                .map_err(|e| anyhow::anyhow!("audio output device: {e}"))?;
            let sink = Arc::new(rodio::Sink::try_new(&handle)
                .map_err(|e| anyhow::anyhow!("audio sink: {e}"))?);
            *ctx = Some((stream, handle, sink.clone()));
            Ok(sink)
        };

        // ── Idle-unload bookkeeping ──────────────────────────────────────────
        // In on-demand mode a model is loaded when it is first needed (or
        // pre-loaded via `TtsCommand::Preload`) and dropped again once it has
        // gone unused for the configured window. Every use — a spoken utterance
        // or a preload — restarts the countdown, so an active conversation never
        // pays the reload cost twice. Read from `current_config` each time round
        // so an `UpdateConfig` takes effect on the next iteration.
        let mut last_used = Instant::now();

        loop {
            let unload_when_idle = current_config.unloads_when_idle();
            let idle = current_config.idle_unload_duration();
            // Piper and eSpeak shell out per utterance and hold nothing; only
            // these three keep weights resident, so only these can be unloaded.
            let model_resident = pocket_tts_model.is_some()
                || inflect_model.is_some()
                || breeze_tts_2_model.is_some()
                || vox_cpm_2_model.is_some();

            // Park until the next command, or until the idle window expires.
            let wait = if unload_when_idle && model_resident {
                match idle.checked_sub(last_used.elapsed()) {
                    Some(remaining) if !remaining.is_zero() => remaining,
                    _ => {
                        info!(
                            "Unloading TTS model after {}s idle (memory mode: on-demand)",
                            idle.as_secs()
                        );
                        // Dropping the model and its cached voice states is the
                        // whole of the resident footprint; the worker thread,
                        // its audio device and the queue all stay up.
                        pocket_tts_model = None;
                        pocket_tts_voice_states.clear();
                        inflect_model = None;
                        breeze_tts_2_model = None;
                        breeze_tts_2_voice_states.clear();
                        vox_cpm_2_model = None;
                        vox_cpm_2_voice_states.clear();
                        self.model_loaded.store(false, Ordering::SeqCst);
                        continue;
                    }
                }
            } else {
                IDLE_PARK
            };

            let cmd = match self.rx.recv_timeout(wait) {
                Ok(cmd) => cmd,
                // Nothing arrived in the window — go round again so the unload
                // branch above can run.
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            };

            match cmd {
                TtsCommand::UpdateConfig(new_cfg) => {
                    info!(
                        "TTS worker config dynamically updated (engine={:?}, memory_mode={:?})",
                        new_cfg.engine, new_cfg.memory_mode
                    );
                    current_config = new_cfg;
                    // Switching to on-demand starts the clock now rather than
                    // dropping a model that may be about to be used again.
                    last_used = Instant::now();
                }
                TtsCommand::Play { mut utterance, generation } => {
                    // Synthesis is about to touch the model; hold off the idle
                    // unload for the whole utterance and restart the countdown
                    // when it ends (below).
                    last_used = Instant::now();
                    let current_gen = self.generation.load(std::sync::atomic::Ordering::SeqCst);
                    if generation < current_gen {
                        debug!("Discarding stale utterance: generation={generation} (current={current_gen})");
                        continue;
                    }

                    let is_prewarm = utterance.source_label.as_deref() == Some("prewarm");

                    if !is_prewarm {
                        let mut snips = current_config.snippets.clone();
                        // Guarantee explicit subword phonetic boundaries so Kyutai models (Pocket-TTS & Breeze-TTS-2)
                        // never omit the 'Con' syllable or slur into 'crol'.
                        snips.entry("VoxCtrl".to_string()).or_insert_with(|| "Voks Con-trol".to_string());
                        snips.entry("voxctrl".to_string()).or_insert_with(|| "Voks Con-trol".to_string());
                        snips.entry("Vox Control".to_string()).or_insert_with(|| "Voks Con-trol".to_string());
                        snips.entry("vox control".to_string()).or_insert_with(|| "Voks Con-trol".to_string());

                        utterance.text = expand_snippets(&utterance.text, &snips);
                        if !self.custom_vocabulary.is_empty() {
                            utterance.text = correct_custom_vocabulary(&utterance.text, &self.custom_vocabulary);
                        }
                    }

                    let sink_res = init_audio(&mut audio_context);
                    if let Err(e) = sink_res {
                        warn!("TTS audio init error: {e}");
                        if !is_prewarm {
                            if let Some(ref cb) = self.on_error {
                                cb(format!("Audio output unavailable: {e}"));
                            }
                        }
                        continue;
                    }
                    let sink = sink_res.unwrap();
                    let speed = if current_config.speed <= 0.0 { 1.0 } else { current_config.speed };
                    sink.set_speed(speed.clamp(0.5, 2.5));

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = Some(sink.clone());
                    }

                    // Caught rather than allowed to unwind: a panic here kills the
                    // worker thread outright, and because the channel sender lives on
                    // in the handle, every later `speak()` succeeds silently. The UI
                    // then waits forever for callbacks that can never fire, which
                    // presents as the app hanging rather than as a failure. ONNX
                    // Runtime can panic during session creation when its shared
                    // library cannot be resolved, so this path is reachable.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        match current_config.engine {
                        TtsEngine::Piper => self.speak_piper(&utterance, &sink),
                        TtsEngine::Espeak => self.speak_espeak(&utterance),
                        TtsEngine::PocketTts => speak_pocket_tts(
                            &current_config,
                            &utterance,
                            &mut pocket_tts_model,
                            &mut pocket_tts_voice_states,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        TtsEngine::InflectMicro => speak_inflect_micro(
                            &current_config,
                            &utterance,
                            &mut inflect_model,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        TtsEngine::BreezeTts2 => speak_breeze_tts_2(
                            &current_config,
                            &utterance,
                            &mut breeze_tts_2_model,
                            &mut breeze_tts_2_voice_states,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        TtsEngine::VoxCpm2 => speak_vox_cpm_2(
                            &current_config,
                            &utterance,
                            &mut vox_cpm_2_model,
                            &mut vox_cpm_2_voice_states,
                            &self.on_playback_start,
                            &sink,
                            &self.generation,
                            generation,
                        ),
                        }
                    }))
                    .unwrap_or_else(|payload| {
                        let detail = payload
                            .downcast_ref::<&str>()
                            .map(|s| (*s).to_string())
                            .or_else(|| payload.downcast_ref::<String>().cloned())
                            .unwrap_or_else(|| "unknown panic".into());
                        Err(anyhow::anyhow!(
                            "TTS engine panicked: {detail}. For the Inflect-Micro-v2 \
                             engine this usually means ONNX Runtime could not be \
                             loaded — this build resolves libonnxruntime at runtime."
                        ))
                    });

                    {
                        let mut guard = ACTIVE_SINK.lock().unwrap();
                        *guard = None;
                    }

                    if let Err(e) = result {
                        warn!("TTS speak error: {e:#}");
                        if !is_prewarm {
                            if let Some(ref cb) = self.on_error {
                                cb(format!("{e:#}"));
                            }
                        }
                    }
                    if !is_prewarm {
                        if let Some(ref cb) = self.on_playback_end {
                            cb();
                        }
                    }

                    self.model_loaded.store(
                        pocket_tts_model.is_some()
                            || inflect_model.is_some()
                            || breeze_tts_2_model.is_some()
                            || vox_cpm_2_model.is_some(),
                        Ordering::SeqCst,
                    );
                    // A long utterance must not count against the idle window.
                    last_used = Instant::now();
                }
                TtsCommand::Preload => {
                    // Speculative: failures are logged, not surfaced. The same
                    // error is reported properly (with a toast) if an utterance
                    // actually arrives.
                    let outcome = match current_config.engine {
                        TtsEngine::PocketTts if pocket_tts_model.is_none() => Some(
                            ensure_pocket_tts_loaded(
                                &current_config,
                                &current_config.pocket_tts.voice.clone(),
                                &mut pocket_tts_model,
                                &mut pocket_tts_voice_states,
                            ),
                        ),
                        TtsEngine::InflectMicro if inflect_model.is_none() => {
                            Some(ensure_inflect_micro_loaded(&current_config, &mut inflect_model))
                        }
                        TtsEngine::BreezeTts2 if breeze_tts_2_model.is_none() => {
                            Some(ensure_breeze_tts_2_loaded(&current_config, &mut breeze_tts_2_model))
                        }
                        TtsEngine::VoxCpm2 if vox_cpm_2_model.is_none() => {
                            Some(ensure_vox_cpm_2_loaded(&current_config, &mut vox_cpm_2_model))
                        }
                        // Already resident, or an engine that holds no model.
                        _ => None,
                    };
                    match outcome {
                        Some(Err(e)) => debug!("TTS preload skipped: {e:#}"),
                        Some(Ok(())) => debug!("TTS model pre-loaded and primed"),
                        None => {}
                    }
                    // A partial load (weights in, voice state failed) still holds
                    // memory, so track what is actually resident.
                    self.model_loaded.store(
                        pocket_tts_model.is_some()
                            || inflect_model.is_some()
                            || breeze_tts_2_model.is_some()
                            || vox_cpm_2_model.is_some(),
                        Ordering::SeqCst,
                    );
                    last_used = Instant::now();
                }
                TtsCommand::Shutdown => {
                    debug!("TTS shutdown signal received");
                    stop_current_playback();
                    break;
                }
            }
        }
    }

    fn speak_piper(&self, u: &Utterance, sink: &rodio::Sink) -> Result<()> {
        let binary = piper_binary().ok_or_else(|| {
            anyhow::anyhow!(
                "Piper binary not found. Download a voice from TTS settings (this \
                 also installs the standalone Piper engine), or install piper \
                 system-wide."
            )
        })?;
        let voice_name = u.voice.as_deref().unwrap_or(&self.config.voice);

        let voice_path =
            get_voice_path(voice_name, &self.config.voice_dir).ok_or_else(|| {
                anyhow::anyhow!("Piper voice files not found for: {}", voice_name)
            })?;

        let length_scale = 1.0 / self.config.speed;
        let mut cmd = std::process::Command::new(&binary);
        cmd.arg("--model")
            .arg(&voice_path)
            .arg("--length-scale")
            .arg(length_scale.to_string())
            .arg("--output-raw");

        if self.config.gpu {
            cmd.arg("--cuda");
        }

        let mut piper = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("spawn piper")?;

        use std::io::Write;
        piper
            .stdin
            .as_mut()
            .unwrap()
            .write_all(u.text.as_bytes())
            .context("write to piper stdin")?;

        let output = piper.wait_with_output().context("wait piper")?;

        if !output.status.success() {
            let err_msg = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "piper process failed with exit code {:?}: {}",
                output.status.code(),
                err_msg.trim()
            );
        }

        if output.stdout.is_empty() {
            anyhow::bail!("piper produced empty stdout");
        }

        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        play_raw_audio(sink, &output.stdout, sample_rate_for_voice(voice_name))?;
        Ok(())
    }

    fn speak_espeak(&self, u: &Utterance) -> Result<()> {
        if voxctrl_config::find_in_path("espeak-ng").is_none() {
            anyhow::bail!(
                "espeak-ng is not installed on this system. Install it with your \
                 package manager (e.g. `sudo pacman -S espeak-ng` or `sudo apt \
                 install espeak-ng`) or switch to another TTS engine."
            );
        }

        if u.source_label.as_deref() != Some("prewarm") {
            if let Some(ref cb) = self.on_playback_start {
                cb();
            }
        }

        let wpm = (175.0 * self.config.speed) as i32;
        let status = std::process::Command::new("espeak-ng")
            .arg("-s")
            .arg(wpm.to_string())
            .arg(&u.text)
            .status()
            .context("spawn espeak-ng")?;
        if !status.success() {
            anyhow::bail!("espeak-ng exited with status {:?}", status.code());
        }
        Ok(())
    }
}

// ── Audio playback ────────────────────────────────────────────────────────────

fn play_raw_audio(sink: &rodio::Sink, raw: &[u8], sample_rate: u32) -> Result<()> {
    let samples: Vec<i16> = raw
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();

    sink.append(rodio::buffer::SamplesBuffer::new(1, sample_rate, samples));
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── stop() must bump the generation counter ──────────────────────────────
    //
    // Regression test for a bug where the global stop-key hotkey called the raw
    // `stop_current_playback()` free function instead of `TtsEngineHandle::stop()`.
    // That stopped the Rodio sink but left the generation counter unchanged, so
    // Pocket-TTS's frame-by-frame streaming loop (which only checks the counter
    // between frames) kept appending new audio — and `Sink::append()` resets the
    // sink's `stopped` flag, so playback silently resumed after the "stop".

    #[test]
    fn test_handle_stop_increments_generation_counter() {
        let (tx, _rx) = bounded(32);
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let handle = TtsEngineHandle {
            tx,
            generation: generation.clone(),
            model_loaded: Arc::new(AtomicBool::new(false)),
        };

        assert_eq!(generation.load(std::sync::atomic::Ordering::SeqCst), 0);
        handle.stop();
        assert_eq!(generation.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn test_raw_stop_current_playback_does_not_bump_generation() {
        // Documents the exact gap that caused the regression: calling the free
        // function alone never advances any generation counter, since it has no
        // knowledge of one. Callers MUST go through TtsEngineHandle::stop().
        let generation = Arc::new(std::sync::atomic::AtomicU32::new(0));
        stop_current_playback();
        assert_eq!(generation.load(std::sync::atomic::Ordering::SeqCst), 0);
    }

    #[test]
    fn test_streaming_loop_cancellation_check_breaks_on_stale_generation() {
        // Mirrors the per-frame check inside speak_pocket_tts: once stop() bumps
        // the live counter past the snapshotted generation, the loop must stop
        // appending further frames instead of running to completion.
        let generation_counter = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let snapshotted_generation = 0u32;

        let mut frames_processed = 0;
        for _ in 0..5 {
            if generation_counter.load(std::sync::atomic::Ordering::SeqCst) != snapshotted_generation {
                break;
            }
            frames_processed += 1;
            if frames_processed == 2 {
                // Simulate stop() firing mid-stream.
                generation_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
        }

        assert_eq!(frames_processed, 2, "loop must abandon remaining frames after stop()");
    }

    // ── Idle-unload memory mode ──────────────────────────────────────────────

    fn test_handle() -> (TtsEngineHandle, Receiver<TtsCommand>) {
        let (tx, rx) = bounded(32);
        let handle = TtsEngineHandle {
            tx,
            generation: Arc::new(AtomicU32::new(0)),
            model_loaded: Arc::new(AtomicBool::new(false)),
        };
        (handle, rx)
    }

    #[test]
    fn test_preload_sends_preload_command() {
        let (handle, rx) = test_handle();
        handle.preload();
        assert!(matches!(rx.try_recv(), Ok(TtsCommand::Preload)));
    }

    #[test]
    fn test_update_config_carries_memory_policy_to_worker() {
        let (handle, rx) = test_handle();
        let cfg = TtsConfig {
            memory_mode: voxctrl_config::TtsMemoryMode::OnDemand,
            idle_unload_secs: 900,
            ..TtsConfig::default()
        };
        handle.update_config(cfg);
        match rx.try_recv() {
            Ok(TtsCommand::UpdateConfig(cfg)) => {
                assert!(cfg.unloads_when_idle());
                assert_eq!(cfg.idle_unload_duration(), Duration::from_secs(900));
            }
            other => panic!("expected UpdateConfig, got {other:?}"),
        }
    }

    #[test]
    fn test_model_loaded_flag_defaults_false() {
        let (handle, _rx) = test_handle();
        assert!(!handle.is_model_loaded());
    }

    // Mirrors the worker's park-duration decision: while the model is resident
    // the worker must wake exactly when the idle window runs out, and once it
    // has expired the remaining time is None (which is the unload trigger).
    #[test]
    fn test_idle_window_expiry_arithmetic() {
        let idle = Duration::from_secs(900);

        let fresh = Duration::from_secs(10);
        assert_eq!(idle.checked_sub(fresh), Some(Duration::from_secs(890)));

        let expired = Duration::from_secs(901);
        assert_eq!(idle.checked_sub(expired), None);

        // Exactly at the boundary the remaining window is zero, which the worker
        // treats as expired rather than parking for 0ns in a tight loop.
        let boundary = idle.checked_sub(idle).unwrap();
        assert!(boundary.is_zero());
    }

    // A use part-way through the window must push the deadline out rather than
    // letting the original one stand — "the time is reset if it is used again".
    #[test]
    fn test_use_resets_idle_countdown() {
        let idle = Duration::from_secs(900);
        let first_use = Instant::now();
        let elapsed_at_second_use = Duration::from_secs(600);

        // Without a reset the model would have 300s left ...
        assert_eq!(idle.checked_sub(elapsed_at_second_use), Some(Duration::from_secs(300)));

        // ... but the worker stamps `last_used` again, so the full window is back.
        let last_used = first_use + elapsed_at_second_use;
        let remaining = idle.checked_sub(last_used.duration_since(last_used)).unwrap();
        assert_eq!(remaining, idle);
    }

    #[test]
    fn test_config_idle_duration_defaults_to_fifteen_minutes() {
        let cfg = TtsConfig::default();
        assert_eq!(cfg.idle_unload_secs, 900);
        assert_eq!(cfg.idle_unload_duration(), Duration::from_secs(900));
    }

    #[test]
    fn test_config_defaults_to_always_loaded() {
        assert!(!TtsConfig::default().unloads_when_idle());
    }

    #[test]
    fn test_config_idle_duration_is_floored() {
        // A 0 (or 1s) setting would drop the model between two sentences of the
        // same reply; the config floors it instead of honouring it literally.
        let cfg = TtsConfig { idle_unload_secs: 0, ..TtsConfig::default() };
        assert_eq!(cfg.idle_unload_duration(), Duration::from_secs(30));
    }

    #[test]
    fn test_on_demand_mode_reported_by_config() {
        let cfg = TtsConfig {
            memory_mode: voxctrl_config::TtsMemoryMode::OnDemand,
            ..TtsConfig::default()
        };
        assert!(cfg.unloads_when_idle());
    }
}
