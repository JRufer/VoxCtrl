use std::{
    collections::HashSet,
    sync::Arc,
    time::{Duration, Instant},
};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use voxctr_routing::{GestureType, HotkeyBinding};

/// Event emitted when a gesture is fully recognized.
#[derive(Debug, Clone)]
pub struct GestureEvent {
    pub binding_id: String,
    pub target_id: String,
    pub kind: GestureKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GestureKind {
    /// Hold started
    Start,
    /// Hold released / toggle activated
    Stop,
}

// ── State machines ────────────────────────────────────────────────────────────

/// Per-binding mutable state.
pub struct BindingState {
    pub binding: HotkeyBinding,
    // Hold
    pub hold_active: bool,
    // Toggle
    pub toggle_on: bool,
    // Double-tap
    pub double_tap: DoubleTapMachine,
}

impl BindingState {
    pub fn new(binding: HotkeyBinding) -> Self {
        let tap_ms = binding.tap_ms;
        Self {
            binding,
            hold_active: false,
            toggle_on: false,
            double_tap: DoubleTapMachine::new(Duration::from_millis(tap_ms as u64)),
        }
    }
}

// ── Double-tap state machine ──────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
enum DtState {
    Idle,
    FirstDown,
    FirstUp,
    SecondDown,
}

pub struct DoubleTapMachine {
    state: DtState,
    deadline: Duration,
    last_up: Option<Instant>,
    cancel: Option<CancellationToken>,
}

impl DoubleTapMachine {
    pub fn new(deadline: Duration) -> Self {
        Self {
            state: DtState::Idle,
            deadline,
            last_up: None,
            cancel: None,
        }
    }

    /// Returns true when a valid double-tap is completed.
    pub fn on_press(&mut self) -> bool {
        match self.state {
            DtState::Idle => {
                self.state = DtState::FirstDown;
                false
            }
            DtState::FirstUp => {
                if let Some(up) = self.last_up {
                    if up.elapsed() <= self.deadline {
                        self.state = DtState::SecondDown;
                        self.cancel_timer();
                        return true;
                    }
                }
                self.state = DtState::FirstDown;
                false
            }
            _ => false,
        }
    }

    pub fn on_release(&mut self) {
        match self.state {
            DtState::FirstDown => {
                self.state = DtState::FirstUp;
                self.last_up = Some(Instant::now());
                self.arm_timer();
            }
            DtState::SecondDown => {
                self.state = DtState::Idle;
            }
            _ => {}
        }
    }

    fn arm_timer(&mut self) {
        let token = CancellationToken::new();
        self.cancel = Some(token.clone());
        let deadline = self.deadline;
        tokio::spawn(async move {
            tokio::select! {
                _ = tokio::time::sleep(deadline) => {}
                _ = token.cancelled() => {}
            }
        });
    }

    fn cancel_timer(&mut self) {
        if let Some(t) = self.cancel.take() {
            t.cancel();
        }
    }

    pub fn reset(&mut self) {
        self.cancel_timer();
        self.state = DtState::Idle;
        self.last_up = None;
    }
}

// ── Superset shadowing ────────────────────────────────────────────────────────

/// Given a set of currently-pressed keys and a list of sorted bindings,
/// filter out bindings whose key set is a proper subset of a longer binding
/// that is also active. This prevents Meta+Space firing when Ctrl+Meta+Space
/// is held.
pub fn shadowed_by_longer(
    pressed: &HashSet<String>,
    bindings: &[BindingState],
) -> HashSet<String> {
    let active: Vec<&BindingState> = bindings
        .iter()
        .filter(|b| !b.binding.disabled)
        .filter(|b| b.binding.keys.iter().all(|k| pressed.contains(k)))
        .collect();

    let mut shadowed = HashSet::new();
    for b in &active {
        for other in &active {
            if b.binding.id != other.binding.id
                && other.binding.keys.len() > b.binding.keys.len()
                && b.binding.keys.iter().all(|k| other.binding.keys.contains(k))
            {
                shadowed.insert(b.binding.id.clone());
            }
        }
    }
    shadowed
}
