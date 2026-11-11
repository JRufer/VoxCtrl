//! Which hold-to-talk keys the user has down right now.
//!
//! Text injection asks this before it synthesises a keystroke. On a desktop
//! where the compositor owns the key grab (the XDG `GlobalShortcuts` portal),
//! a keystroke VoxCtrl sends while the user is holding a dictation key ends
//! that dictation: KDE reports a held shortcut released as soon as *any* key
//! is released, VoxCtrl's synthetic `V` included, and never reports it active
//! again until the key is physically pressed afresh. Nothing after the fact
//! can repair that — the real release is not reported either — so the only
//! fix is not to send the keystroke until the key is up.
//!
//! A process global rather than state handed to each backend: the gesture
//! engine is built separately inside every backend, and the one consumer
//! lives in another crate. It is keyed by binding id, so engines (and tests)
//! that share the process do not disturb each other's entries.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A key recorded as down for longer than this is a release that went
/// missing, not a hold. Slightly past `gestures::STUCK_HOLD_MAX`, which ends
/// the recording itself — and clears the entry — before this is reached; this
/// only covers a press that never became a recording.
const STALE_AFTER: Duration = Duration::from_secs(125);

static DOWN: Mutex<Option<HashMap<String, Instant>>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut HashMap<String, Instant>) -> R) -> R {
    let mut guard = DOWN.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// The trigger of hold-style binding `id` went down.
pub(crate) fn pressed(id: &str) {
    with(|m| {
        m.entry(id.to_string()).or_insert_with(Instant::now);
    });
}

/// The trigger of binding `id` is no longer down, or its hold is over.
pub(crate) fn released(id: &str) {
    with(|m| {
        m.remove(id);
    });
}

/// Is the trigger of binding `id` recorded as down?
pub(crate) fn is_down(id: &str) -> bool {
    with(|m| m.get(id).is_some_and(|at| at.elapsed() < STALE_AFTER))
}

/// True while the user is holding a `hold` or `double_tap_hold` key: from the
/// press — before the hold threshold, so before recording has started — until
/// the release, or until the hold's own safety timeout gives up on it.
pub fn hold_key_down() -> bool {
    with(|m| {
        m.retain(|_, at| at.elapsed() < STALE_AFTER);
        !m.is_empty()
    })
}
