use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use tauri::Emitter;
use tokio::sync::Mutex;
use voxctrl_hotkeys::GestureKind;

use crate::state::AppState;
#[cfg(target_os = "linux")]
use crate::tray::update_tray_for_setup;
use crate::window::{setup_blocker, show_setup_window, SETUP_NOTICE_INTERVAL};
// Only `spawn_setup_watcher` uses these, and it is Linux-only: it polls for a
// desktop portal that has bound nothing, a failure the Windows hook cannot have.
#[cfg(target_os = "linux")]
use crate::window::{BLIND_ALERT_INTERVAL, SETUP_POLL_INTERVAL};
use voxctrl_inference::InferenceOutput;

/// Finish a remote (streamed) transcription the way the local worker finishes
/// its own — same noise gate, post-processing and per-hotkey rewrite, from the
/// shared `voxctrl_inference::finalize` — and hand the result to delivery.
async fn process_remote_transcription(
    res: anyhow::Result<voxctrl_inference::backend::TranscriptionResult>,
    audio: Vec<f32>,
    ctx: RecordingContext,
    state: Arc<AppState>,
    text_tx: crossbeam_channel::Sender<InferenceOutput>,
) {
    use voxctrl_inference::finalize;
    let RecordingContext { target_id, binding_id, processing, binding } = ctx;

    let result = match res {
        Ok(res) => res,
        Err(e) => {
            let _ = text_tx.send(InferenceOutput::failed(target_id, Some(binding_id), &e, false, 0));
            return;
        }
    };

    let app_cfg = state.config.lock().await.data.clone();

    // ── Noise Gate (VAD) ──────────────────────────────────────────────────
    let rms = finalize::rms(&audio);
    let rms_threshold = finalize::noise_gate_threshold(app_cfg.audio.vad_threshold);
    if rms < rms_threshold {
        tracing::info!(
            "Remote audio skipped by noise gate: RMS is {rms:.5} (threshold is {rms_threshold:.5}, vad_threshold={:.2})",
            app_cfg.audio.vad_threshold
        );
        let _ = text_tx.send(InferenceOutput {
            inference_ms: result.inference_ms,
            language: result.language,
            ..InferenceOutput::empty(target_id, Some(binding_id), false, 0)
        });
        return;
    }

    let mut processed = finalize::post_process(&result.text, rms, &processing, &app_cfg);

    // ── Hotkey-Specific OpenAI Post-Processing ────────────────────────────
    if !processed.is_empty() {
        if let Some(openai_cfg) = finalize::binding_openai_config(binding.as_ref(), &app_cfg.openai) {
            processed = voxctrl_llm::OpenAiClient::new(openai_cfg).process(&processed).await;
        }
    }

    let _ = text_tx.send(InferenceOutput {
        text: processed,
        target_id,
        binding_id: Some(binding_id),
        raw_text: result.text,
        inference_ms: result.inference_ms,
        language: result.language,
        error: None,
        is_interim: false,
        session_id: 0,
    });
}

/// Who a recording is for, captured once when it starts: where it goes, which
/// hotkey started it, and the settings those imply for finishing it. Taken
/// from the in-memory caches, so no config file is read per recording.
#[derive(Clone, Default)]
struct RecordingContext {
    target_id: String,
    binding_id: String,
    /// The primary target's post-processing overrides.
    processing: voxctrl_routing::TargetProcessingConfig,
    /// The saved binding behind `binding_id`, if any.
    binding: Option<voxctrl_routing::HotkeyBinding>,
}

impl RecordingContext {
    /// Snapshot the active target and hotkey. Blocking: pipeline threads only.
    fn capture(state: &AppState) -> Self {
        let target_id = state.active_target.blocking_lock().clone();
        let binding_id = state.active_binding_id.blocking_lock().clone();
        Self {
            processing: voxctrl_inference::finalize::target_processing(
                &target_id,
                &state.targets.blocking_lock(),
            ),
            binding: state.binding(&binding_id),
            target_id,
            binding_id,
        }
    }

    fn request(
        &self,
        audio: Vec<f32>,
        is_interim: bool,
        session_id: u64,
    ) -> voxctrl_inference::InferenceRequest {
        voxctrl_inference::InferenceRequest {
            audio,
            target_id: self.target_id.clone(),
            binding_id: Some(self.binding_id.clone()),
            is_interim,
            session_id,
            processing: self.processing.clone(),
            binding: self.binding.clone(),
        }
    }
}

/// Without live text, interim passes only feed early command detection, and
/// only the opening of a recording is transcribed: the trigger and the target
/// name come first ("Hey Vox, add this to my notes"), and capping the window
/// keeps each pass short, so the final transcription never waits long behind
/// one and long dictations don't keep the model busy the whole time.
/// (With live text, `live_text.rs` decides what each pass covers.)
const INTERIM_MAX_SAMPLES: usize = 6 * 16_000;

/// Whether the current speech engine can produce live text mid-recording:
/// a local engine that isn't a heavy Whisper model running without a GPU
/// (each pass would take longer than the speech it transcribes).
pub fn live_text_capable(cfg: &voxctrl_config::AppConfig) -> bool {
    let heavy_model = cfg.engine.backend == voxctrl_config::BackendChoice::WhisperCpp
        && voxctrl_inference::whisper_gpu_backend().is_none()
        && (cfg.engine.whisper_cpp.model_size.starts_with("medium")
            || cfg.engine.whisper_cpp.model_size.starts_with("large"));
    cfg.engine.backend != voxctrl_config::BackendChoice::RemoteOpenAi && !heavy_model
}

/// Whether the overlay should get live text this recording: the user wants
/// it, the engine can make it, and the overlay style shows it (any built-in
/// style, or a custom overlay with a live-text element).
fn live_text_for(cfg: &voxctrl_config::AppConfig) -> bool {
    cfg.ui.show_overlay
        && cfg.ui.show_live_text
        && cfg.ui.overlay_style != "none"
        && live_text_capable(cfg)
        && crate::commands::overlay_has_live_text(&cfg.ui.overlay_style)
}

pub fn spawn_audio_coordinator(
    state_for_audio: Arc<AppState>,
    audio_rx: crossbeam_channel::Receiver<voxctrl_audio::AudioChunk>,
    inference_tx: crossbeam_channel::Sender<voxctrl_inference::InferenceRequest>,
    text_tx: crossbeam_channel::Sender<voxctrl_inference::InferenceOutput>,
    rt_handle: tokio::runtime::Handle,
) {
    std::thread::spawn(move || {
        let mut accumulated_audio = Vec::<f32>::new();
        let mut was_recording = false;
        let mut ctx = RecordingContext::default();
        let mut remote_session: Option<voxctrl_inference::RemoteStreamingSession> = None;
        let mut is_remote_backend = false;
        let mut interim_enabled = false;
        let mut live_text_enabled = false;
        let mut session_id: u64 = 0;
        let mut last_interim_sample_count: usize = 0;
        let mut last_interim_instant = std::time::Instant::now();

        while let Ok(chunk) = audio_rx.recv() {
            let is_recording = state_for_audio.is_recording();

            if is_recording {
                if !was_recording {
                    accumulated_audio.clear();
                    ctx = RecordingContext::capture(&state_for_audio);
                    was_recording = true;
                    session_id = session_id.wrapping_add(1);
                    last_interim_sample_count = 0;
                    last_interim_instant = std::time::Instant::now();
                    state_for_audio.set_interim_in_flight(false);

                    let cfg = state_for_audio.config.blocking_lock().data.clone();
                    // Early command detection only pays off when a command
                    // could match (some non-router target exists) and the
                    // local model is fast enough to transcribe mid-recording.
                    let has_command_targets = state_for_audio
                        .targets
                        .blocking_lock()
                        .iter()
                        .any(|t| t.delivery != voxctrl_routing::DeliveryType::Command);
                    let commands_enabled = cfg.features.early_command_detection
                        && live_text_capable(&cfg)
                        && has_command_targets;
                    // Live text is only worth its transcription cost while an
                    // overlay that shows it is on screen.
                    live_text_enabled = live_text_for(&cfg);
                    state_for_audio
                        .live_text_wanted
                        .store(live_text_enabled, std::sync::atomic::Ordering::SeqCst);
                    *state_for_audio.live_session.lock().unwrap_or_else(|e| e.into_inner()) =
                        crate::live_text::LiveSession::new(session_id);
                    interim_enabled = commands_enabled || live_text_enabled;
                    state_for_audio
                        .interim_for_commands
                        .store(commands_enabled, std::sync::atomic::Ordering::SeqCst);
                    if cfg.engine.backend == voxctrl_config::BackendChoice::RemoteOpenAi {
                        is_remote_backend = true;
                        let prompt = Some(voxctrl_inference::finalize::initial_prompt(
                            &cfg.features.custom_vocabulary,
                        ));
                        let session = voxctrl_inference::RemoteStreamingSession::start(
                            cfg.engine.remote_openai.clone(),
                            prompt,
                            &rt_handle,
                        );
                        remote_session = Some(session);
                    } else {
                        is_remote_backend = false;
                        remote_session = None;
                    }
                }

                if let Some(ref session) = remote_session {
                    session.send_chunk(chunk.clone());
                }
                accumulated_audio.extend(chunk);

                // Mid-recording passes: live text for the overlay and/or early
                // detection of a voice command while the user is still talking.
                let total = accumulated_audio.len();
                if interim_enabled
                    && crate::live_text::pass_due(
                        total,
                        last_interim_sample_count,
                        last_interim_instant.elapsed().as_millis(),
                        crate::live_text::pass_interval_ms(
                            state_for_audio.last_interim_ms.load(std::sync::atomic::Ordering::Relaxed),
                        ),
                        state_for_audio.is_interim_in_flight(),
                    )
                {
                    // Live text transcribes the uncommitted tail (or commits a
                    // piece of it); command detection alone reads the opening.
                    let range = if live_text_enabled {
                        let mut live = state_for_audio.live_session.lock().unwrap_or_else(|e| e.into_inner());
                        live.start_pass(&accumulated_audio).0
                    } else {
                        0..total.min(INTERIM_MAX_SAMPLES)
                    };
                    // Command detection has read everything it needs once the
                    // opening is covered; without live text, stop there.
                    if live_text_enabled || range.end > last_interim_sample_count.min(INTERIM_MAX_SAMPLES) {
                        state_for_audio.set_interim_in_flight(true);
                        last_interim_sample_count = total;
                        last_interim_instant = std::time::Instant::now();
                        state_for_audio
                            .interim_from_start
                            .store(range.start == 0, std::sync::atomic::Ordering::SeqCst);
                        let _ = inference_tx.send(ctx.request(
                            accumulated_audio[range].to_vec(),
                            true,
                            session_id,
                        ));
                    }
                }
            } else {
                if was_recording {
                    if is_remote_backend {
                        if let Some(session) = remote_session.take() {
                            let audio = std::mem::take(&mut accumulated_audio);
                            if !audio.is_empty() {
                                state_for_audio.set_processing(true);
                                let state_clone = state_for_audio.clone();
                                let text_tx_clone = text_tx.clone();
                                let ctx = ctx.clone();

                                rt_handle.spawn(async move {
                                    let res = session.finish().await;
                                    process_remote_transcription(
                                        res,
                                        audio,
                                        ctx,
                                        state_clone,
                                        text_tx_clone,
                                    )
                                    .await;
                                });
                            }
                        }
                    } else {
                        if !accumulated_audio.is_empty() {
                            let req = ctx.request(
                                std::mem::take(&mut accumulated_audio),
                                false,
                                session_id,
                            );
                            state_for_audio.set_processing(true);
                            let _ = inference_tx.send(req);
                        }
                    }
                    was_recording = false;
                }
            }
        }
    });
}

fn live_text_wanted_now(state: &AppState) -> bool {
    state.live_text_wanted.load(std::sync::atomic::Ordering::SeqCst)
}

/// Push the live transcript to the overlay: committed words, tentative ones,
/// and the whole text for overlays written before the split existed. `final`
/// marks the finished transcript replacing the live guess.
fn emit_live_transcript(session_id: u64, view: &crate::live_text::LiveView, is_final: bool) {
    if let Some(app) = crate::window::get_app_handle() {
        let _ = app.emit(
            "live-transcript",
            serde_json::json!({
                "session_id": session_id,
                "committed": view.committed,
                "tentative": view.tentative,
                "text": view.full_text(),
                "final": is_final,
            }),
        );
    }
}

pub fn spawn_text_delivery_worker(
    state: Arc<AppState>,
    text_rx: crossbeam_channel::Receiver<voxctrl_inference::InferenceOutput>,
    rt_handle: tokio::runtime::Handle,
) {
    std::thread::spawn(move || {
        // (session, target) already announced from an interim pass, so the
        // overlay and TTS preload fire once per command, not once per pass.
        let mut announced: Option<(u64, String)> = None;

        while let Ok(output) = text_rx.recv() {
            if output.is_interim {
                state.set_interim_in_flight(false);
                state.last_interim_ms.store(output.inference_ms, std::sync::atomic::Ordering::Relaxed);
                // Live text: fold this pass into the session's committed and
                // tentative words. An empty result still counts (a commit
                // pass over a pause moves the tail on); a failed one doesn't.
                if live_text_wanted_now(&state) {
                    let mut live = state.live_session.lock().unwrap_or_else(|e| e.into_inner());
                    if live.session_id == output.session_id {
                        if output.error.is_some() {
                            live.abandon_pass();
                        } else {
                            let view = live.finish_pass(output.text.trim()).clone();
                            emit_live_transcript(output.session_id, &view, false);
                        }
                    }
                }
                if output.error.is_some() || output.text.trim().is_empty() {
                    continue;
                }
                // Early command detection: surface the command overlay and
                // start loading the TTS model while the user is still talking.
                // This is only a head start — routing is decided solely by the
                // final transcript below, which must carry the trigger itself.
                let commands = state.interim_for_commands.load(std::sync::atomic::Ordering::SeqCst)
                    && state.interim_from_start.load(std::sync::atomic::Ordering::SeqCst);
                if !commands {
                    continue;
                }
                let targets = state.targets.blocking_lock().clone();
                if let Some(parsed) = voxctrl_routing::targets::parse_voice_command(&output.text, &targets) {
                    let key = (output.session_id, parsed.matched_target_id);
                    if announced.as_ref() != Some(&key) {
                        let matched_target = targets.iter().find(|t| t.id == key.1);
                        let matched_label = voxctrl_routing::targets_display_label(&key.1, &targets);
                        tracing::info!("Early command detected during speech: '{matched_label}' (target: {})", key.1);
                        voxctrl_routing::targets::notify_command_trigger(&matched_label, &parsed.payload);

                        let leads_to_speech = matched_target.is_some_and(|t| {
                            t.delivery == voxctrl_routing::DeliveryType::Speak
                                || t.response_pipe.as_deref().is_some_and(|p| !p.trim().is_empty())
                        });
                        if leads_to_speech {
                            let state_c = state.clone();
                            rt_handle.spawn(async move { state_c.preload_tts().await });
                        }
                        announced = Some(key);
                    }
                }
                continue;
            }

            state.set_interim_in_flight(false);
            state.set_processing(false);

            // The final transcript replaces the live guess in the overlay,
            // committed in full, before it goes away.
            if live_text_wanted_now(&state) && output.error.is_none() {
                let mut live = state.live_session.lock().unwrap_or_else(|e| e.into_inner());
                if live.session_id == output.session_id {
                    live.abandon_pass();
                    let view = crate::live_text::LiveView {
                        committed: output.text.trim().to_string(),
                        tentative: String::new(),
                    };
                    emit_live_transcript(output.session_id, &view, true);
                }
            }

            // A command announced from an interim pass stands only if the
            // final transcript confirms it; otherwise take its overlay down.
            let announced_early = announced.take().is_some_and(|(session, _)| session == output.session_id);
            let withdraw_early_command = || {
                if announced_early {
                    voxctrl_routing::targets::notify_command_withdrawn();
                }
            };

            if let Some(ref err) = output.error {
                withdraw_early_command();
                // Always surface transcription failures — without this a
                // fresh install with no Whisper model records audio and
                // then silently drops it, which reads as "hotkeys broken".
                tracing::error!("Transcription failed: {err}");
                voxctrl_inject::show_notification("VoxCtrl — transcription failed", err);
                continue;
            }
            if output.text.trim().is_empty() {
                withdraw_early_command();
                continue;
            }

            // ── S1-mini & Command Processing ──────────────────────────────────
            // Command resolution runs first to prevent S1-mini from altering the
            // trigger word or command syntax. If a command matches, the remaining
            // text (payload) is cleaned by S1-mini before delivery to the matched target.
            // If dictation starts with the trigger word but does not match a command
            // (e.g. "VoxCtrl is a great app."), the full sentence including the
            // trigger word is retained as the result text and cleaned by S1-mini.
            let (global_s1_mini_enabled, s1_mini_styling, s1_mini_gpu, show_notif) = {
                let cfg_lock = state.config.blocking_lock();
                (
                    cfg_lock.data.engine.s1_mini.enabled,
                    cfg_lock.data.engine.s1_mini.styling.clone(),
                    cfg_lock.data.engine.s1_mini.gpu,
                    cfg_lock.data.ui.show_notification,
                )
            };

            let s1_mini_enabled = output
                .binding_id
                .as_deref()
                .and_then(|bid| state.binding(bid))
                .and_then(|b| b.s1_mini_enabled)
                .unwrap_or(global_s1_mini_enabled);

            // The in-memory cache `save_targets` keeps current — the same set
            // the router delivers to, so a matched target is always deliverable.
            let targets = state.targets.blocking_lock().clone();
            let (target_id, raw_text) = if let Some(parsed) = voxctrl_routing::targets::parse_voice_command(&output.text, &targets) {
                let matched_label =
                    voxctrl_routing::targets_display_label(&parsed.matched_target_id, &targets);
                voxctrl_routing::targets::notify_command_trigger(&matched_label, &parsed.payload);
                (parsed.matched_target_id, parsed.payload)
            } else {
                withdraw_early_command();
                (output.target_id.clone(), output.text.clone())
            };

            // Text bound for a Speak target is read aloud verbatim: S1-mini
            // would add latency before the first word and may rewrite what
            // the user meant to have spoken.
            let is_speak_target = targets
                .iter()
                .any(|t| t.id == target_id && t.delivery == voxctrl_routing::DeliveryType::Speak);
            let text = if !is_speak_target && s1_mini_enabled && !raw_text.trim().is_empty() {
                voxctrl_inference::s1_mini::clean_dictation(&raw_text, &s1_mini_styling, None, s1_mini_gpu)
            } else {
                raw_text
            };

            if text.trim().is_empty() {
                continue;
            }

            tracing::info!(
                "Received transcription: \"{}\" for target '{}' (took {}ms)",
                text,
                target_id,
                output.inference_ms
            );
            let words = text.split_whitespace().count() as u32;
            state.increment_words(words);

            // Deliver text via the output target router.
            // Write last_text BEFORE launching deliveries so that MCP
            // transcribe_voice can detect the result without waiting for
            // potentially slow targets (webhooks, sockets, etc.).
            let router = state.router.clone();
            let state_lt = state.clone();
            let text_lt = text.clone();
            let text_to_deliver = text.clone();
            let target_to_deliver = target_id.clone();
            rt_handle.spawn(async move {
                {
                    let mut lt = state_lt.last_text.lock().await;
                    *lt = text_lt;
                    state_lt
                        .last_text_version
                        .fetch_add(1, Ordering::SeqCst);
                }
                let target_ids: Vec<String> = target_to_deliver
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                for tid in target_ids {
                    router.deliver_direct(&tid, &text_to_deliver).await;
                }
            });

            if show_notif {
                voxctrl_inject::show_notification("VoxCtrl", &text);
            }
        }
    });
}

pub fn spawn_hotkey_gesture_handler(
    state_for_gesture: Arc<AppState>,
    gesture_rx: voxctrl_hotkeys::GestureReceiver,
) {
    // Notices for the silent failure modes of a fresh install: dictating with
    // an unfinished setup, and recording with a mic stream that never delivers
    // audio. The setup notice repeats (throttled) rather than firing once —
    // a single toast at the very first keypress is easy to miss, and the user
    // will keep pressing the shortcut until something explains itself.
    //
    // They live out here so a restarted loop does not start nagging again.
    let last_setup_notice = Arc::new(Mutex::new(None::<std::time::Instant>));
    let mic_notice_shown = Arc::new(AtomicBool::new(false));
    let gesture_rx = Arc::new(Mutex::new(gesture_rx));

    // Supervised, because every shortcut in the app is delivered by this one
    // task: a panic anywhere inside it — in a notification, an injection, a
    // lock — would take every hotkey down with it for the rest of the session,
    // silently, with the app still running and apparently healthy. That is
    // indistinguishable from a desktop that stopped delivering shortcuts, and
    // it is the shape of "my keybind stopped working and a restart fixed it".
    tokio::spawn(async move {
        loop {
            let task = tokio::spawn(run_gesture_loop(
                state_for_gesture.clone(),
                gesture_rx.clone(),
                last_setup_notice.clone(),
                mic_notice_shown.clone(),
            ));
            match task.await {
                Ok(()) => {
                    tracing::warn!(
                        "Hotkey gesture loop ended: the listener's channel closed. No \
                         shortcut can fire until VoxCtrl is restarted."
                    );
                    return;
                }
                Err(e) if e.is_panic() => {
                    tracing::error!(
                        "Hotkey gesture loop panicked ({e}); restarting it so shortcuts \
                         keep working. Please report this with the backtrace above."
                    );
                }
                Err(e) => {
                    tracing::warn!("Hotkey gesture loop stopped: {e}");
                    return;
                }
            }
        }
    });
}

async fn run_gesture_loop(
    state_for_gesture: Arc<AppState>,
    gesture_rx: Arc<Mutex<voxctrl_hotkeys::GestureReceiver>>,
    last_setup_notice: Arc<Mutex<Option<std::time::Instant>>>,
    mic_notice_shown: Arc<AtomicBool>,
) {
    // Held for the life of the loop; a restart takes it back and carries on
    // with whatever the listener queued in between.
    let mut gesture_rx = gesture_rx.lock().await;
    while let Some(event) = gesture_rx.recv().await {
        // TTS stop key: fires on key-down (Start), not release.
        // Only stop the active Rodio sink — do NOT send None to the worker
        // channel (that would kill the thread and break all future TTS).
        if event.binding_id == voxctrl_routing::TTS_STOP_BINDING_ID {
            if event.kind == GestureKind::Start {
                // Use the handle's stop() (not the raw stop_current_playback())
                // so the generation counter is bumped too — otherwise a
                // streaming engine like Pocket-TTS keeps appending the
                // frames it already had in flight and audio resumes.
                if let Some(tts) = state_for_gesture.tts_handle.lock().await.as_ref() {
                    tts.stop();
                }
            }
            continue;
        }

        match event.kind {
            GestureKind::Start => {
                // Drop gestures while the keybind recorder is open. The user
                // is pressing keys to configure a new binding — acting on them
                // would start an unwanted dictation session mid-setup.
                if state_for_gesture.is_hotkeys_inhibited() {
                    tracing::debug!(
                        "Hotkey '{}' gesture suppressed: keybind recorder is active",
                        event.binding_id
                    );
                    continue;
                }

                *state_for_gesture.active_target.lock().await = event.target_id.clone();
                *state_for_gesture.active_binding_label.lock().await =
                    event.binding_label.clone();
                *state_for_gesture.active_binding_id.lock().await = event.binding_id.clone();
                state_for_gesture.begin_recording().await;

                // Earliest point at which we know speech is probably coming.
                // In the on-demand memory mode the model is not resident, so
                // start pulling it in now — the load runs while the user is
                // still talking instead of after they stop.
                let preload_state = state_for_gesture.clone();
                let preload_target = event.target_id.clone();
                tokio::spawn(async move {
                    preload_state.preload_tts_for_target(&preload_target).await;
                });

                // The user just tried to dictate. If the install is not
                // finished, say so now — otherwise the shortcut records
                // audio that can never become text, which reads as
                // "VoxCtrl is broken" rather than "setup is incomplete".
                if let Some(msg) = setup_blocker(&state_for_gesture).await {
                    let now = std::time::Instant::now();
                    let stale = {
                        let last = last_setup_notice.lock().await;
                        last.map(|t: std::time::Instant| {
                            now.duration_since(t) > SETUP_NOTICE_INTERVAL
                        })
                        .unwrap_or(true)
                    };
                    if stale {
                        *last_setup_notice.lock().await = Some(now);
                        voxctrl_inject::show_notification("VoxCtrl — setup unfinished", &msg);
                        show_setup_window();
                    }
                }

                // Warn if the microphone stream never comes up while the
                // user is recording (dead input device, audio stack issue).
                let st = state_for_gesture.clone();
                let mic_shown = mic_notice_shown.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    if st.is_recording()
                        && !st.is_audio_ready()
                        && !mic_shown.swap(true, Ordering::SeqCst)
                    {
                        voxctrl_inject::show_notification(
                            "VoxCtrl",
                            "Recording is active but no microphone audio is arriving. Check the input device in Settings → Audio.",
                        );
                    }
                });
            }
            GestureKind::Stop => {
                state_for_gesture.set_recording(false);
            }
        }
    }
}

#[cfg(target_os = "linux")]
pub fn spawn_setup_watcher(
    app_handle: tauri::AppHandle,
    health: Arc<voxctrl_hotkeys::ListenerHealth>,
) {
    tauri::async_runtime::spawn(async move {
        // Give the listener one scan before judging it.
        tokio::time::sleep(Duration::from_millis(500)).await;

        let mut first_pass = true;
        let mut was_active: Option<bool> = None;
        let mut last_alert: Option<std::time::Instant> = None;
        loop {
            // Deliberately only the listener's own health: it is a
            // pair of atomics, and "a keyboard is open right now"
            // is the ground truth anyway. Re-deriving it from udev
            // rules and group lookups would spawn subprocesses on
            // every tick for the life of the app, and would still
            // be a worse answer.
            let active = health.is_active();

            if active && was_active == Some(false) {
                voxctrl_inject::show_notification(
                    "VoxCtrl",
                    "Your global shortcuts are registered and working now.",
                );
            }

            if !active {
                // No shortcut can reach the app in this state, so
                // the alert cannot wait for a keypress. The window
                // is the alert the first time; after that a toast,
                // since the window may be buried behind whatever
                // the user is actually working in.
                let due = last_alert
                    .map(|t| t.elapsed() >= BLIND_ALERT_INTERVAL)
                    .unwrap_or(true);
                if due {
                    last_alert = Some(std::time::Instant::now());
                    if first_pass {
                        show_setup_window();
                    } else {
                        voxctrl_inject::show_notification(
                            "VoxCtrl — global shortcuts unavailable",
                            "Nothing on this desktop can deliver VoxCtrl's \
                             shortcuts, so pressing them does nothing. Open \
                             VoxCtrl to see why.",
                        );
                    }
                }
            } else if first_pass && crate::commands::missing_injection_tool().is_some() {
                // Hotkeys work but nothing can be typed anywhere —
                // just as broken, and just as invisible.
                show_setup_window();
            }

            if was_active != Some(active) {
                was_active = Some(active);
                let _ = app_handle.emit("setup-status-changed", active);
                update_tray_for_setup(&app_handle, active);
            }

            first_pass = false;
            tokio::time::sleep(SETUP_POLL_INTERVAL).await;
        }
    });
}

pub fn spawn_audio_level_forwarder(
    handle: tauri::AppHandle,
    state_for_audio_level: Arc<AppState>,
    audio_level_rx: crossbeam_channel::Receiver<f32>,
) {
    // The capture callback produces a level for every buffer the device
    // delivers — well over a hundred a second on a small buffer size. Nothing
    // downstream can show more than the display refreshes, so levels are
    // coalesced to this interval: the newest value wins and the ones that
    // arrived in between are dropped instead of each costing a Tauri event,
    // a JSON encode and a message to the overlay process.
    const MIN_INTERVAL: Duration = Duration::from_millis(16);

    // How often the overlay window's always-on-top state is re-sent to the
    // window manager while a dictation is active — a window manager
    // restacking the overlay behind another window never recovers on its
    // own, so this has to be reasserted periodically, not just once at
    // window creation. A no-op before the overlay window exists.
    const TOPMOST_REASSERT_INTERVAL: Duration = Duration::from_secs(1);

    std::thread::spawn(move || {
        let mut last_sent = std::time::Instant::now() - MIN_INTERVAL;
        let mut last_topmost_reassert = std::time::Instant::now() - TOPMOST_REASSERT_INTERVAL;
        while let Ok(mut level) = audio_level_rx.recv() {
            // Drain whatever queued up behind this one and keep the latest.
            while let Ok(newer) = audio_level_rx.try_recv() {
                level = newer;
            }
            let now = std::time::Instant::now();
            if now.duration_since(last_sent) < MIN_INTERVAL {
                continue;
            }
            last_sent = now;

            let _ = handle.emit("audio-level", level);

            // The overlay window gets recording/processing/speaking state and
            // the target label from the status-tick / audio-level Tauri
            // events directly (see src/lib/Overlay/Overlay.svelte) — only the
            // always-on-top reassertion below still needs it here.
            let overlay_on = state_for_audio_level.is_overlay_enabled();
            let is_recording = overlay_on && state_for_audio_level.is_recording();
            let is_processing = overlay_on && state_for_audio_level.is_processing();
            let is_speaking = overlay_on && state_for_audio_level.is_speaking();

            if (is_recording || is_processing || is_speaking)
                && now.duration_since(last_topmost_reassert) >= TOPMOST_REASSERT_INTERVAL
            {
                last_topmost_reassert = now;
                crate::window::reassert_overlay_topmost(&handle);
            }
        }
    });
}

