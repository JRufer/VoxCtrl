//! Whether the remote speech server can be reached right now.
//!
//! One process-wide answer, because several parts of the app need the same
//! one: the recording pipeline (send this dictation to the server, or straight
//! to the fallback engine?), the inference worker (may the fallback model be
//! unloaded yet?), and the tray and Settings (show the offline indicator?).
//!
//! It is written by whoever last learned something — a background probe, or a
//! dictation that just succeeded or failed — and read by everyone else.
//! [`subscribe`] wakes a listener only when the answer actually changes.

use std::sync::OnceLock;

use serde::Serialize;
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteState {
    /// The selected speech engine is a local one; there is no server to reach.
    NotInUse,
    /// A remote engine is selected and nothing has been heard from it yet.
    Unknown,
    /// The server answered the last time it was asked.
    Online,
    /// The server could not be reached, or refused the request.
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RemoteStatus {
    pub state: RemoteState,
    /// Why the server counts as offline, worded for the user. `None` unless
    /// `state` is [`RemoteState::Offline`].
    pub reason: Option<String>,
}

impl RemoteStatus {
    const fn new(state: RemoteState) -> Self {
        Self { state, reason: None }
    }
}

fn channel() -> &'static watch::Sender<RemoteStatus> {
    static CHANNEL: OnceLock<watch::Sender<RemoteStatus>> = OnceLock::new();
    CHANNEL.get_or_init(|| watch::channel(RemoteStatus::new(RemoteState::NotInUse)).0)
}

fn set(status: RemoteStatus) {
    channel().send_if_modified(|current| {
        if *current == status {
            false
        } else {
            *current = status;
            true
        }
    });
}

pub fn current() -> RemoteStatus {
    channel().borrow().clone()
}

/// A receiver that is notified each time the status changes.
pub fn subscribe() -> watch::Receiver<RemoteStatus> {
    channel().subscribe()
}

pub fn is_online() -> bool {
    current().state == RemoteState::Online
}

pub fn is_offline() -> bool {
    current().state == RemoteState::Offline
}

pub fn set_not_in_use() {
    set(RemoteStatus::new(RemoteState::NotInUse));
}

/// A remote engine was just selected (or its address changed): whatever was
/// known about the previous one no longer applies.
pub fn set_unknown() {
    set(RemoteStatus::new(RemoteState::Unknown));
}

pub fn set_online() {
    set(RemoteStatus::new(RemoteState::Online));
}

pub fn set_offline(reason: impl Into<String>) {
    set(RemoteStatus { state: RemoteState::Offline, reason: Some(reason.into()) });
}

/// Held by every test in this crate that writes the status: it is one value
/// for the whole process, and tests run in parallel.
#[cfg(test)]
pub(crate) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    /// One test, because the status is process-wide: several tests writing it
    /// in parallel would see each other's values.
    #[test]
    fn status_is_shared_and_listeners_wake_only_on_change() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut rx = subscribe();
        rx.mark_unchanged();

        set_online();
        assert!(is_online());
        assert!(!is_offline());
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();

        // The same answer again is not news.
        set_online();
        assert!(!rx.has_changed().unwrap());

        set_offline("Could not connect to 192.168.1.50:8787");
        assert!(is_offline());
        assert_eq!(current().reason.as_deref(), Some("Could not connect to 192.168.1.50:8787"));
        assert!(rx.has_changed().unwrap());
        rx.mark_unchanged();

        // A different reason is a change: the tooltip should follow it.
        set_offline("The server rejected the API key (HTTP 401)");
        assert!(rx.has_changed().unwrap());

        set_unknown();
        assert_eq!(current(), RemoteStatus { state: RemoteState::Unknown, reason: None });
        set_not_in_use();
        assert_eq!(current().state, RemoteState::NotInUse);
        assert!(!is_online() && !is_offline());
    }

    #[test]
    fn serializes_for_the_frontend() {
        let json = serde_json::to_value(RemoteStatus {
            state: RemoteState::Offline,
            reason: Some("down".into()),
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "state": "offline", "reason": "down" }));
        assert_eq!(
            serde_json::to_value(RemoteState::NotInUse).unwrap(),
            serde_json::json!("not_in_use")
        );
    }
}
