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

use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Something the user should know about a server that is working: that
    /// it runs an older VoxCtrl than this app expects, say. Only ever set
    /// while `state` is [`RemoteState::Online`].
    pub notice: Option<String>,
}

impl RemoteStatus {
    const fn new(state: RemoteState) -> Self {
        Self { state, reason: None, notice: None }
    }
}

/// Set while the server answers but speaks a newer client contract than this
/// app (see `remote_openai::compatibility`). A dictation that happens to work
/// must not mark such a server online again: only the next check of the
/// server's version may.
static INCOMPATIBLE: AtomicBool = AtomicBool::new(false);

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
    INCOMPATIBLE.store(false, Ordering::SeqCst);
    set(RemoteStatus::new(RemoteState::NotInUse));
}

/// A remote engine was just selected (or its address changed): whatever was
/// known about the previous one no longer applies.
pub fn set_unknown() {
    INCOMPATIBLE.store(false, Ordering::SeqCst);
    set(RemoteStatus::new(RemoteState::Unknown));
}

/// The server just answered a request. Keeps a notice the last check of the
/// server found, and changes nothing while the server is known to be
/// incompatible with this app.
pub fn set_online() {
    if INCOMPATIBLE.load(Ordering::SeqCst) {
        return;
    }
    let notice = {
        let current = channel().borrow();
        (current.state == RemoteState::Online).then(|| current.notice.clone()).flatten()
    };
    set(RemoteStatus { state: RemoteState::Online, reason: None, notice });
}

/// The background check found the server reachable and compatible; `notice`
/// is what it found worth telling the user, if anything.
pub fn set_checked(notice: Option<String>) {
    INCOMPATIBLE.store(false, Ordering::SeqCst);
    set(RemoteStatus { state: RemoteState::Online, reason: None, notice });
}

pub fn set_offline(reason: impl Into<String>) {
    set(RemoteStatus { state: RemoteState::Offline, reason: Some(reason.into()), notice: None });
}

/// The server answers but this app cannot work with it: offline, with
/// `reason`, until a check of the server says otherwise.
pub fn set_incompatible(reason: impl Into<String>) {
    INCOMPATIBLE.store(true, Ordering::SeqCst);
    set_offline(reason);
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

        // A notice from the version check survives a successful dictation.
        set_checked(Some("The server runs VoxCtrl 0.9.10".into()));
        set_online();
        assert_eq!(current().notice.as_deref(), Some("The server runs VoxCtrl 0.9.10"));

        // An incompatible server stays offline through a dictation that
        // worked, until the version check clears it.
        set_incompatible("Update VoxCtrl");
        set_online();
        assert!(is_offline());
        assert_eq!(current().reason.as_deref(), Some("Update VoxCtrl"));
        set_checked(None);
        assert!(is_online());
        set_incompatible("Update VoxCtrl");

        // Choosing another server forgets all of it.
        set_unknown();
        assert_eq!(current(), RemoteStatus { state: RemoteState::Unknown, reason: None, notice: None });
        set_online();
        assert!(is_online());
        set_not_in_use();
        assert_eq!(current().state, RemoteState::NotInUse);
        assert!(!is_online() && !is_offline());
    }

    #[test]
    fn serializes_for_the_frontend() {
        let json = serde_json::to_value(RemoteStatus {
            state: RemoteState::Offline,
            reason: Some("down".into()),
            notice: None,
        })
        .unwrap();
        assert_eq!(json, serde_json::json!({ "state": "offline", "reason": "down", "notice": null }));
        assert_eq!(
            serde_json::to_value(RemoteState::NotInUse).unwrap(),
            serde_json::json!("not_in_use")
        );
    }
}
