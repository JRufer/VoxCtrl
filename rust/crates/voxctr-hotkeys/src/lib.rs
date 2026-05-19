pub mod gestures;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use tracing::info;
use voxctr_routing::HotkeyBinding;

pub use gestures::{GestureEvent, GestureKind};

/// Callback channel: the listener sends GestureEvents to the app coordinator.
pub type GestureSender = mpsc::UnboundedSender<GestureEvent>;
pub type GestureReceiver = mpsc::UnboundedReceiver<GestureEvent>;

pub fn channel() -> (GestureSender, GestureReceiver) {
    mpsc::unbounded_channel()
}

/// Start the platform-specific hotkey listener on a dedicated OS thread.
/// Bindings can be updated at runtime via `reload_bindings`.
pub fn start_listener(
    bindings: Vec<HotkeyBinding>,
    tx: GestureSender,
    device_path: Option<String>,
) -> ListenerHandle {
    #[cfg(target_os = "linux")]
    {
        linux::start(bindings, tx, device_path)
    }
    #[cfg(target_os = "windows")]
    {
        windows::start(bindings, tx)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        tracing::warn!("Hotkey listener not supported on this platform");
        ListenerHandle { _inner: () }
    }
}

/// Opaque handle; drop to stop the listener.
pub struct ListenerHandle {
    #[allow(dead_code)]
    _inner: std::marker::PhantomData<()>,
}
