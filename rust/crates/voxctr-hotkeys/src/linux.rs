use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use tracing::{debug, info, warn};
use voxctr_routing::{GestureType, HotkeyBinding};

use crate::{
    gestures::{shadowed_by_longer, BindingState, GestureEvent, GestureKind},
    GestureSender, ListenerHandle,
};

pub fn start(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    device_path: Option<String>,
) -> ListenerHandle {
    std::thread::Builder::new()
        .name("voxctr-evdev".into())
        .spawn(move || run(bindings, tx, device_path))
        .expect("failed to spawn evdev thread");
    ListenerHandle {
        _inner: std::marker::PhantomData,
    }
}

fn run(bindings: Vec<HotkeyBinding>, tx: GestureSender, device_path: Option<String>) {
    let device = open_device(&device_path);
    let mut device = match device {
        Some(d) => d,
        None => {
            warn!("No suitable keyboard evdev device found; hotkeys disabled");
            return;
        }
    };

    info!("evdev listener active on {}", device.name().unwrap_or("unknown"));

    let mut states: Vec<BindingState> =
        bindings.into_iter().map(BindingState::new).collect();
    let mut pressed: HashSet<String> = HashSet::new();

    loop {
        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if ev.event_type() != evdev::EventType::KEY {
                        continue;
                    }
                    let key_name = format!("{:?}", ev.code());
                    let value = ev.value();
                    // value: 1 = press, 0 = release, 2 = repeat
                    if value == 1 {
                        pressed.insert(key_name.clone());
                        handle_press(&key_name, &mut states, &pressed, &tx);
                    } else if value == 0 {
                        handle_release(&key_name, &mut states, &pressed, &tx);
                        pressed.remove(&key_name);
                    }
                }
            }
            Err(e) => {
                warn!("evdev read error: {e}; retrying in 1s");
                std::thread::sleep(Duration::from_secs(1));
            }
        }
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
        // Only handle if this key is part of the binding and all binding keys
        // are pressed.
        if !s.binding.keys.contains(&key.to_string()) {
            continue;
        }
        if !s.binding.keys.iter().all(|k| pressed.contains(k)) {
            continue;
        }

        match s.binding.gesture {
            GestureType::Hold => {
                if !s.hold_active {
                    s.hold_active = true;
                    debug!(id = %s.binding.id, "hold start");
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        target_id: s.binding.target_id.clone(),
                        kind: GestureKind::Start,
                    });
                }
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
                // Chord fires immediately when all keys are down (no hold required)
                let _ = tx.send(GestureEvent {
                    binding_id: s.binding.id.clone(),
                    target_id: s.binding.target_id.clone(),
                    kind: GestureKind::Start,
                });
            }
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
        if s.binding.disabled {
            continue;
        }
        if !s.binding.keys.contains(&key.to_string()) {
            continue;
        }

        match s.binding.gesture {
            GestureType::Hold => {
                if s.hold_active {
                    s.hold_active = false;
                    debug!(id = %s.binding.id, "hold stop");
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        target_id: s.binding.target_id.clone(),
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

// ── Device selection ──────────────────────────────────────────────────────────

fn open_device(preferred: &Option<String>) -> Option<evdev::Device> {
    if let Some(path) = preferred {
        if let Ok(d) = evdev::Device::open(path) {
            return Some(d);
        }
        warn!("Saved evdev device {path} not accessible; auto-detecting");
    }

    // Auto-detect: pick the first non-virtual keyboard that has typical keys
    let mut candidates: Vec<(u32, evdev::Device)> = evdev::enumerate()
        .filter_map(|(path, dev)| {
            let name = dev.name().unwrap_or("").to_ascii_lowercase();
            // Skip virtual devices (uinput, xtest, etc.)
            if name.contains("virtual")
                || name.contains("uinput")
                || name.contains("xtest")
            {
                return None;
            }
            // Must have KEY capability
            let has_keys = dev.supported_keys().is_some();
            if has_keys {
                let score = if name.contains("keyboard") { 10 } else { 1 };
                Some((score, dev))
            } else {
                None
            }
        })
        .collect();

    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.into_iter().next().map(|(_, d)| d)
}
