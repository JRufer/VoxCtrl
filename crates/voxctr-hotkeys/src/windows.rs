use std::{collections::HashSet, time::Duration};

use tracing::{debug, info, warn};
use voxctr_routing::{GestureType, HotkeyBinding};

use crate::{
    gestures::{shadowed_by_longer, BindingState, GestureEvent, GestureKind},
    GestureSender, ListenerHandle,
};

pub fn start(bindings: Vec<HotkeyBinding>, tx: GestureSender) -> ListenerHandle {
    std::thread::Builder::new()
        .name("voxctr-rdev".into())
        .spawn(move || run(bindings, tx))
        .expect("failed to spawn rdev thread");
    ListenerHandle {
        _inner: std::marker::PhantomData,
    }
}

fn run(bindings: Vec<HotkeyBinding>, tx: GestureSender) {
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
        move |event: rdev::Event| {
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
            GestureType::Hold if !s.hold_active => {
                s.hold_active = true;
                let _ = tx.send(GestureEvent {
                    binding_id: s.binding.id.clone(),
                    target_id: s.binding.target_id.clone(),
                    kind: GestureKind::Start,
                });
            }
            GestureType::Toggle => {
                if !s.toggle_on {
                    s.toggle_on = true;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        target_id: s.binding.target_id.clone(),
                        kind: GestureKind::Start,
                    });
                } else {
                    s.toggle_on = false;
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        target_id: s.binding.target_id.clone(),
                        kind: GestureKind::Stop,
                    });
                }
            }
            GestureType::DoubleTap => {
                if s.double_tap.on_press() {
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        target_id: s.binding.target_id.clone(),
                        kind: GestureKind::Start,
                    });
                }
            }
            GestureType::Chord => {
                let _ = tx.send(GestureEvent {
                    binding_id: s.binding.id.clone(),
                    target_id: s.binding.target_id.clone(),
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
    pressed: &HashSet<String>,
    tx: &GestureSender,
) {
    for s in states.iter_mut() {
        if s.binding.disabled || !s.binding.keys.contains(&key.to_string()) {
            continue;
        }
        match s.binding.gesture {
            GestureType::Hold if s.hold_active => {
                s.hold_active = false;
                let _ = tx.send(GestureEvent {
                    binding_id: s.binding.id.clone(),
                    target_id: s.binding.target_id.clone(),
                    kind: GestureKind::Stop,
                });
            }
            GestureType::DoubleTap => {
                s.double_tap.on_release();
            }
            _ => {}
        }
    }
}
