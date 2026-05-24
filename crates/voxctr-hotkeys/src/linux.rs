use std::{
    collections::HashSet,
    time::Duration,
};

use tracing::warn;
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
    let rt_handle = tokio::runtime::Handle::try_current().ok();

    if let Some(path) = device_path {
        let b = bindings.clone();
        let t = tx.clone();
        let rt = rt_handle.clone();
        std::thread::Builder::new()
            .name("voxctr-evdev".into())
            .spawn(move || {
                let _guard = rt.as_ref().map(|h| h.enter());
                run(b, t, Some(path))
            })
            .expect("failed to spawn evdev thread");
    } else {
        let candidates = find_all_keyboards();
        if candidates.is_empty() {
            warn!("No suitable keyboard evdev device found; hotkeys disabled");
        } else {
            for path in candidates {
                let b = bindings.clone();
                let t = tx.clone();
                let p = Some(path);
                let rt = rt_handle.clone();
                std::thread::Builder::new()
                    .name("voxctr-evdev".into())
                    .spawn(move || {
                        let _guard = rt.as_ref().map(|h| h.enter());
                        run(b, t, p)
                    })
                    .expect("failed to spawn evdev thread");
            }
        }
    }

    ListenerHandle {
        _inner: std::marker::PhantomData,
    }
}

fn run(bindings: Vec<HotkeyBinding>, tx: GestureSender, device_path: Option<String>) {
    let mut device = match open_device(&device_path) {
        Some(d) => d,
        None => return,
    };




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
                    let mut key_name = match ev.kind() {
                        evdev::InputEventKind::Key(key) => format!("{:?}", key),
                        _ => format!("{:?}", ev.code()),
                    };
                    if key_name.starts_with("Key(") && key_name.ends_with(')') {
                        key_name = key_name[4..key_name.len() - 1].to_string();
                    }
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
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Start,
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
                let completed = s.double_tap.on_press();
                if completed {
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Start,
                    });
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
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Stop,
                    });
                }
            }
            GestureType::DoubleTap => {
                let completed = s.double_tap.on_release();
                if completed {
                    let _ = tx.send(GestureEvent {
                        binding_id: s.binding.id.clone(),
                        binding_label: s.binding.label.clone(),
                        target_id: s.binding.target_ids_string(),
                        kind: GestureKind::Stop,
                    });
                }
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
        warn!("Saved evdev device {path} not accessible");
    }
    None
}

fn find_all_keyboards() -> Vec<String> {
    evdev::enumerate()
        .filter_map(|(path, dev)| {
            let name = dev.name().unwrap_or("").to_ascii_lowercase();
            // Skip virtual and passthrough devices (uinput, xtest, passthrough, etc.)
            if name.contains("virtual")
                || name.contains("uinput")
                || name.contains("xtest")
                || name.contains("passthrough")
            {
                return None;
            }
            // Must have KEY capability and support standard keyboard keys like KEY_A
            let has_keys = dev.supported_keys()
                .map(|keys| keys.contains(evdev::Key::KEY_A))
                .unwrap_or(false);
            if has_keys {
                tracing::info!("Found eligible keyboard: {} at {:?}", name, path);
                Some(path.to_string_lossy().to_string())
            } else {
                None
            }
        })
        .collect()
}
