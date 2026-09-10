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

async fn process_remote_transcription(
    res: anyhow::Result<voxctrl_inference::backend::TranscriptionResult>,
    audio: Vec<f32>,
    target_id: String,
    binding_id: String,
    state: Arc<AppState>,
    text_tx: crossbeam_channel::Sender<voxctrl_inference::InferenceOutput>,
) {
    let result = match res {
        Ok(res) => res,
        Err(e) => {
            let _ = text_tx.send(InferenceOutput {
                text: String::new(),
                target_id,
                raw_text: String::new(),
                inference_ms: 0,
                language: String::new(),
                error: Some(format!("{e:#}")),
            });
            return;
        }
    };

    // ── Noise Gate (VAD) ──────────────────────────────────────────────────
    let sum_sq: f32 = audio.iter().map(|&s| s * s).sum();
    let rms = if audio.is_empty() {
        0.0
    } else {
        (sum_sq / audio.len() as f32).sqrt()
    };
    let vad_threshold = {
        let guard = state.config.lock().await;
        guard.data.audio.vad_threshold
    };
    let rms_threshold = (1.0 - vad_threshold) * 0.006;

    if rms < rms_threshold {
        tracing::info!(
            "Remote audio skipped by noise gate: RMS is {:.5} (threshold is {:.5}, vad_threshold={:.2})",
            rms,
            rms_threshold,
            vad_threshold
        );
        let _ = text_tx.send(InferenceOutput {
            text: String::new(),
            target_id,
            raw_text: String::new(),
            inference_ms: result.inference_ms,
            language: result.language,
            error: None,
        });
        return;
    }

    let dir = voxctrl_routing::config_dir();
    let targets = voxctrl_routing::load_targets(&dir).unwrap_or_default();
    let target_ids: Vec<&str> = target_id
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    let first_target_id = target_ids.first().copied().unwrap_or("default");
    let target = targets.iter().find(|t| t.id == first_target_id);

    let app_cfg = {
        let guard = state.config.lock().await;
        guard.data.clone()
    };

    let remove_fillers = target
        .and_then(|t| t.processing.remove_fillers)
        .unwrap_or(app_cfg.features.remove_fillers);

    let spoken_punctuation = target
        .and_then(|t| t.processing.spoken_punctuation)
        .unwrap_or(app_cfg.features.spoken_punctuation);

    let auto_format_lists = target
        .and_then(|t| t.processing.auto_format_lists)
        .unwrap_or(app_cfg.features.auto_format_lists);

    let code_mode = target
        .and_then(|t| t.processing.code_mode)
        .unwrap_or(false);

    let post_cfg = voxctrl_inference::postprocess::PostProcessConfig {
        remove_fillers,
        spoken_punctuation,
        auto_format_lists,
        apply_snippets: !app_cfg.features.snippets.is_empty(),
        snippets: &app_cfg.features.snippets,
        code_mode,
        custom_vocabulary: &app_cfg.features.custom_vocabulary,
    };

    let mut processed = voxctrl_inference::postprocess::run_pipeline(&result.text, &post_cfg);
    let raw_text = result.text.clone();

    // ── Silence Hallucination Filter ──────────────────────────────────────
    if !processed.is_empty()
        && voxctrl_inference::postprocess::is_silence_hallucination(&processed)
        && rms < 0.003
    {
        tracing::info!(
            "Discarded silence hallucination '{}' (audio RMS: {:.5})",
            processed,
            rms
        );
        processed = String::new();
    }

    // ── Hotkey-Specific OpenAI Post-Processing ────────────────────────────
    let bindings = voxctrl_routing::load_bindings(&dir).unwrap_or_default();
    let binding = bindings.iter().find(|b| b.id == binding_id);

    let binding_wants_openai = binding
        .and_then(|b| b.openai_enabled)
        .unwrap_or(false);

    if binding_wants_openai && !processed.is_empty() {
        let mut openai_cfg = voxctrl_config::Config::load().data.openai;
        openai_cfg.enabled = true;

        if let Some(b) = binding {
            if let Some(ref model) = b.openai_model {
                if !model.is_empty() {
                    openai_cfg.model = model.clone();
                }
            }
            if let Some(ref mode_str) = b.openai_mode {
                let mode = match mode_str.as_str() {
                    "clean" => voxctrl_config::OpenAiMode::Clean,
                    "formal" => voxctrl_config::OpenAiMode::Formal,
                    "casual" => voxctrl_config::OpenAiMode::Casual,
                    "bullet" => voxctrl_config::OpenAiMode::Bullet,
                    "concise" => voxctrl_config::OpenAiMode::Concise,
                    "custom" => voxctrl_config::OpenAiMode::Custom,
                    _ => voxctrl_config::OpenAiMode::Clean,
                };
                if mode != voxctrl_config::OpenAiMode::Custom {
                    openai_cfg.mode = mode.clone();
                    openai_cfg.system_prompt =
                        voxctrl_llm::preset_system_prompt(&mode).to_string();
                }
            }
            if let Some(ref system_prompt) = b.openai_system_prompt {
                if !system_prompt.is_empty() {
                    openai_cfg.system_prompt = system_prompt.clone();
                }
            }
            if let Some(ref prompt) = b.openai_prompt {
                if !prompt.is_empty() {
                    openai_cfg.user_prompt = prompt.clone();
                }
            }
        }

        let client = voxctrl_llm::OpenAiClient::new(openai_cfg);
        processed = client.process(&processed).await;
    }

    let _ = text_tx.send(InferenceOutput {
        text: processed,
        target_id,
        raw_text,
        inference_ms: result.inference_ms,
        language: result.language,
        error: None,
    });
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
        let mut target_id = "default".to_string();
        let mut binding_id = String::new();
        let mut remote_session: Option<voxctrl_inference::RemoteStreamingSession> = None;
        let mut is_remote_backend = false;

        while let Ok(chunk) = audio_rx.recv() {
            let is_recording = state_for_audio.is_recording();

            if is_recording {
                if !was_recording {
                    accumulated_audio.clear();
                    target_id = state_for_audio.active_target.blocking_lock().clone();
                    binding_id = state_for_audio.active_binding_id.blocking_lock().clone();
                    was_recording = true;

                    let cfg = state_for_audio.config.blocking_lock().data.clone();
                    if cfg.engine.backend == voxctrl_config::BackendChoice::RemoteOpenAi {
                        is_remote_backend = true;
                        let mut merged_prompt = String::from(
                            "VoxCtrl is a voice control assistant application. VoxCtrl commands start with VoxCtrl. ",
                        );
                        if !cfg.features.custom_vocabulary.is_empty() {
                            merged_prompt.push_str("Vocabulary: ");
                            merged_prompt.push_str(&cfg.features.custom_vocabulary.join(", "));
                            merged_prompt.push_str(". ");
                        }
                        let prompt = {
                            let end = merged_prompt.trim_end().len();
                            merged_prompt.truncate(end);
                            (!merged_prompt.is_empty()).then_some(merged_prompt)
                        };
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
            } else {
                if was_recording {
                    if is_remote_backend {
                        if let Some(session) = remote_session.take() {
                            let audio = std::mem::take(&mut accumulated_audio);
                            if !audio.is_empty() {
                                state_for_audio.set_processing(true);
                                let state_clone = state_for_audio.clone();
                                let text_tx_clone = text_tx.clone();
                                let target_id_clone = target_id.clone();
                                let binding_id_clone = binding_id.clone();

                                rt_handle.spawn(async move {
                                    let res = session.finish().await;
                                    process_remote_transcription(
                                        res,
                                        audio,
                                        target_id_clone,
                                        binding_id_clone,
                                        state_clone,
                                        text_tx_clone,
                                    )
                                    .await;
                                });
                            }
                        }
                    } else {
                        if !accumulated_audio.is_empty() {
                            let req = voxctrl_inference::InferenceRequest {
                                audio: std::mem::take(&mut accumulated_audio),
                                target_id: target_id.clone(),
                                binding_id: Some(binding_id.clone()),
                            };
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

pub fn spawn_text_delivery_worker(
    state: Arc<AppState>,
    text_rx: crossbeam_channel::Receiver<voxctrl_inference::InferenceOutput>,
    rt_handle: tokio::runtime::Handle,
) {
    std::thread::spawn(move || {
        while let Ok(output) = text_rx.recv() {
            state.set_processing(false);
            if let Some(ref err) = output.error {
                // Always surface transcription failures — without this a
                // fresh install with no Whisper model records audio and
                // then silently drops it, which reads as "hotkeys broken".
                tracing::error!("Transcription failed: {err}");
                voxctrl_inject::show_notification("VoxCtrl — transcription failed", err);
                continue;
            }
            if output.text.trim().is_empty() {
                continue;
            }
            tracing::info!(
                "Received transcription: \"{}\" for target '{}' (took {}ms)",
                output.text,
                output.target_id,
                output.inference_ms
            );
            let words = output.text.split_whitespace().count() as u32;
            state.increment_words(words);

            // Deliver text via the output target router.
            // Write last_text BEFORE launching deliveries so that MCP
            // transcribe_voice can detect the result without waiting for
            // potentially slow targets (webhooks, sockets, etc.).
            let text = output.text.clone();
            let target_id = output.target_id.clone();
            let router = state.router.clone();
            let state_lt = state.clone();
            let text_lt = output.text.clone();
            rt_handle.spawn(async move {
                {
                    let mut lt = state_lt.last_text.lock().await;
                    *lt = text_lt;
                    state_lt
                        .last_text_version
                        .fetch_add(1, Ordering::SeqCst);
                }
                let target_ids: Vec<String> = target_id
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                for tid in target_ids {
                    router.deliver(&tid, &text).await;
                }
            });

            let show_notif = {
                let cfg_lock = state.config.blocking_lock();
                cfg_lock.data.ui.show_notification
            };
            if show_notif {
                voxctrl_inject::show_notification("VoxCtrl", &output.text);
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

    std::thread::spawn(move || {
        let mut last_sent = std::time::Instant::now() - MIN_INTERVAL;
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

            // Forward to Slint overlay channel. When the overlay is
            // disabled, report idle state so the native window never maps
            // (a mapped overlay steals keyboard focus on Wayland and breaks
            // text injection).
            let overlay_on = state_for_audio_level.is_overlay_enabled();
            let is_recording = overlay_on && state_for_audio_level.is_recording();
            let is_processing = overlay_on && state_for_audio_level.is_processing();
            let is_speaking = overlay_on && state_for_audio_level.is_speaking();
            let audio_ready = state_for_audio_level.is_audio_ready();
            let active_target_label = match state_for_audio_level.active_binding_label.try_lock() {
                Ok(label) if !label.is_empty() => label.clone(),
                _ => "Focused Window".to_string(),
            };

            let msg = serde_json::json!({
                "type": "status",
                "recording": is_recording,
                "processing": is_processing,
                "speaking": is_speaking,
                "audio_ready": audio_ready,
                "audio_level": level,
                "active_target_label": active_target_label,
            });

            if let Ok(json_str) = serde_json::to_string(&msg) {
                let _ = state_for_audio_level.overlay_tx.send(json_str);
            }
        }
    });
}
