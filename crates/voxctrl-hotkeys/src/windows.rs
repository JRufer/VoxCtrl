use std::{collections::HashSet, time::Duration};

use tracing::{debug, info, warn};
use voxctrl_routing::{GestureType, HotkeyBinding};

use crate::{
    gestures::{shadowed_by_longer, BindingState, GestureEvent, GestureKind},
    GestureSender, ListenerHandle,
};

pub fn start(bindings: Vec<HotkeyBinding>, tx: GestureSender, rx_reload: crate::ReloaderReceiver) {
    std::thread::Builder::new()
        .name("voxctrl-rdev".into())
        .spawn(move || run(bindings, tx, rx_reload))
        .expect("failed to spawn rdev thread");
}

fn run(bindings: Vec<HotkeyBinding>, tx: GestureSender, rx_reload: crate::ReloaderReceiver) {
    info!("rdev hotkey listener active (Windows)");

    let mut states: Vec<BindingState> =
        bindings.into_iter().map(BindingState::new).collect();
    let mut pressed: HashSet<String> = HashSet::new();

    // rdev callback-based API; we convert to our key names.
    let tx = std::sync::Arc::new(std::sync::Mutex::new(tx));
    let states = std::sync::Arc::new(std::sync::Mutex::new(states));
    let pressed = std::sync::Arc::new(std::sync::Mutex::new(pressed));

    let cb = {
        let tx = tx.clone();
        let states = states.clone();
        let pressed = pressed.clone();
        let rx_reload = rx_reload.clone();
        move |event: rdev::Event| {
            if let Ok(new_bindings) = rx_reload.try_recv() {
                tracing::info!("windows hotkey loop: reloading {} bindings", new_bindings.len());
                let mut st = states.lock().unwrap();
                let mut pr = pressed.lock().unwrap();
                *st = new_bindings.into_iter().map(BindingState::new).collect();
                pr.clear();
            }

            let key_name = match &event.event_type {
                rdev::EventType::KeyPress(k) => format!("KEY_{k:?}").to_ascii_uppercase(),
                rdev::EventType::KeyRelease(k) => format!("KEY_{k:?}").to_ascii_uppercase(),
                _ => return,
            };

            let is_press = matches!(event.event_type, rdev::EventType::KeyPress(_));
            let tx = tx.lock().unwrap();
            let mut st = states.lock().unwrap();
            let mut pr = pressed.lock().unwrap();

            if is_press {
                pr.insert(key_name.clone());
                handle_press(&key_name, &mut st, &pr, &tx);
            } else {
                handle_release(&key_name, &mut st, &pr, &tx);
                pr.remove(&key_name);
            }
        }
    };

    if let Err(e) = rdev::listen(cb) {
        warn!("rdev listener error: {e:?}");
    }
}

fn handle_press(
    key: &str,
    states: &mut Vec<BindingState>,
    pressed: &HashSet<String>,
    tx: &GestureSender,
) {
    let shadowed = shadowed_by_longer(pressed, states);
    for s in states.iter_mut() {
        if s.binding.disabled || shadowed.contains(&s.binding.id) {
            continue;
        }
        if !s.binding.keys.contains(&key.to_string()) {
            continue;
        }
        if !s.binding.keys.iter().all(|k| pressed.contains(k)) {
            continue;
        }
        match s.binding.gesture {
            GestureType::Hold => {
                if !s.hold_active.load(std::sync::atomic::Ordering::SeqCst) && s.hold_cancel.is_none() {
                    let cancel = tokio_util::sync::CancellationToken::new();
                    s.hold_cancel = Some(cancel.clone());
                    let hold_active = s.hold_active.clone();
                    let tx = tx.clone();
                    let binding_id = s.binding.id.clone();
                    let binding_label = s.binding.label.clone();
                    let target_id = s.binding.target_ids_string();
                    let threshold = Duration::from_millis(s.binding.hold_threshold_ms as u64);
                    tokio::spawn(async move {
                        tokio::select! {
                            _ = tokio::time::sleep(threshold) => {
                                hold_active.store(true, std::sync::atomic::Ordering::SeqCst);
                                let _ = tx.send(GestureEvent {
                                    binding_id,
                                    binding_label,
                                    target_id,
                                    kind: GestureKind::Start,
                                });
                            }
                            _ = cancel.cancelled() => {}
                        }
                    });
                }
            }
            GestureType::Toggle => {
                if !s.toggle_on {
                    s.toggle_on = true;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Start,
                    });
                } else {
                    s.toggle_on = false;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Stop,
                    });
                }
            }
            GestureType::DoubleTap => {
                if s.double_tap.on_press() {
                    if !s.toggle_on {
                        s.toggle_on = true;
                        let _ = tx.send(GestureEvent {
                            binding_id: s.binding.id.clone(),
                            binding_label: s.binding.label.clone(),
                            target_id: s.binding.target_ids_string(),
                            kind: GestureKind::Start,
                        });
                    } else {
                        s.toggle_on = false;
                        let _ = tx.send(GestureEvent {
                            binding_id: s.binding.id.clone(),
                            binding_label: s.binding.label.clone(),
                            target_id: s.binding.target_ids_string(),
                            kind: GestureKind::Stop,
                        });
                    }
                }
            }
            GestureType::Chord => {
                let _ = tx.send(GestureEvent {
                    binding_id: s.binding.id.clone(),
                    binding_label: s.binding.label.clone(),
                    target_id: s.binding.target_ids_string(),
                    kind: GestureKind::Start,
                });
            }
            _ => {}
        }
    }
}

fn handle_release(
    key: &str,
    states: &mut Vec<BindingState>,
    _pressed: &HashSet<String>,
    tx: &GestureSender,
) {
    for s in states.iter_mut() {
        if s.binding.disabled || !s.binding.keys.contains(&key.to_string()) {
            continue;
        }
        match s.binding.gesture {
            GestureType::Hold => {
                if let Some(cancel) = s.hold_cancel.take() {
                    cancel.cancel();
                }
                if s.hold_active.swap(false, std::sync::atomic::Ordering::SeqCst) {
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Stop,
                    });
                }
            }
            GestureType::DoubleTap => {
                s.double_tap.on_release();
            }
            _ => {}
        }
    }
}
