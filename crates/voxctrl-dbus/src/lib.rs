use std::sync::Arc;

use anyhow::Result;
use tokio::sync::Mutex;
#[cfg(target_os = "linux")]
use tracing::info;

// ── Shared state ──────────────────────────────────────────────────────────────

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub enum DictationStatus {
    #[default]
    Idle,
    Recording,
    Transcribing,
}

impl std::fmt::Display for DictationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DictationStatus::Idle => write!(f, "idle"),
            DictationStatus::Recording => write!(f, "recording"),
            DictationStatus::Transcribing => write!(f, "transcribing"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AppState {
    pub status: DictationStatus,
    pub word_count: u32,
}

/// How `CheckForUpdate` hands its question to the app and gets the answer
/// back: the newer version found, an empty string when this is the latest
/// release, or why the check failed.
pub type UpdateReply = tokio::sync::oneshot::Sender<std::result::Result<String, String>>;

// ── DBus service (Linux only) ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use zbus::{interface, Connection, SignalContext};

    pub struct DictationInterface {
        pub state: Arc<Mutex<AppState>>,
        /// Channels to send control commands back to the app coordinator. The
        /// start channel carries the id of the binding that asked for it, so a
        /// desktop shortcut can dictate into that binding's own targets. Empty
        /// means "whichever binding the app would use by default".
        pub start_tx: tokio::sync::mpsc::Sender<String>,
        pub stop_tx: tokio::sync::mpsc::Sender<()>,
        /// Asks the app to check GitHub for a newer release.
        pub update_tx: tokio::sync::mpsc::Sender<UpdateReply>,
    }

    #[interface(name = "ai.voxctrl.Dictation")]
    impl DictationInterface {
        async fn start_recording(&self) -> zbus::fdo::Result<()> {
            let _ = self.start_tx.send(String::new()).await;
            Ok(())
        }

        async fn stop_recording(&self) -> zbus::fdo::Result<()> {
            let _ = self.stop_tx.send(()).await;
            Ok(())
        }

        async fn toggle_recording(&self) -> zbus::fdo::Result<()> {
            self.toggle(String::new()).await
        }

        /// Toggle dictation for one specific binding.
        ///
        /// This is what a Cinnamon/MATE native shortcut calls: the desktop owns
        /// the key grab there, so the only way VoxCtrl learns *which* of the
        /// user's bindings fired — and therefore which targets the text goes to
        /// — is for the shortcut to name it.
        async fn toggle_binding(&self, binding_id: String) -> zbus::fdo::Result<()> {
            self.toggle(binding_id).await
        }

        async fn get_status(&self) -> zbus::fdo::Result<String> {
            let guard = self.state.lock().await;
            Ok(guard.status.to_string())
        }

        async fn get_word_count(&self) -> zbus::fdo::Result<u32> {
            let guard = self.state.lock().await;
            Ok(guard.word_count)
        }

        /// Ask GitHub now whether a newer release is published. Returns the new
        /// version, or an empty string when this is the latest. Works whether
        /// or not automatic checks are on: a script calling it is the user
        /// asking. Found updates also appear in the tray.
        async fn check_for_update(&self) -> zbus::fdo::Result<String> {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.update_tx
                .send(tx)
                .await
                .map_err(|_| zbus::fdo::Error::Failed("VoxCtrl is shutting down".into()))?;
            rx.await
                .map_err(|_| zbus::fdo::Error::Failed("The update check was dropped".into()))?
                .map_err(zbus::fdo::Error::Failed)
        }

        /// A check — manual, automatic or `CheckForUpdate` — found a newer
        /// release. Carries its version (`0.9.0`).
        #[zbus(signal)]
        async fn update_available(ctx: &SignalContext<'_>, version: &str) -> zbus::Result<()>;

        #[zbus(signal)]
        async fn status_changed(ctx: &SignalContext<'_>, status: &str) -> zbus::Result<()>;

        #[zbus(signal)]
        async fn text_injected(ctx: &SignalContext<'_>, text: &str) -> zbus::Result<()>;
    }

    impl DictationInterface {
        /// Start `binding_id` if idle, stop whatever is running if not.
        async fn toggle(&self, binding_id: String) -> zbus::fdo::Result<()> {
            let status = {
                let guard = self.state.lock().await;
                guard.status.clone()
            };
            if status == DictationStatus::Recording {
                let _ = self.stop_tx.send(()).await;
            } else {
                let _ = self.start_tx.send(binding_id).await;
            }
            Ok(())
        }
    }

    pub async fn start_service(
        state: Arc<Mutex<AppState>>,
        start_tx: tokio::sync::mpsc::Sender<String>,
        stop_tx: tokio::sync::mpsc::Sender<()>,
        update_tx: tokio::sync::mpsc::Sender<UpdateReply>,
    ) -> Result<Connection> {
        let iface = DictationInterface {
            state,
            start_tx,
            stop_tx,
            update_tx,
        };

        let conn = Connection::session().await?;
        conn.object_server()
            .at("/ai/voxctrl/Dictation", iface)
            .await?;
        conn.request_name("ai.voxctrl.Dictation").await?;
        info!("DBus service registered: ai.voxctrl.Dictation");
        Ok(conn)
    }

    pub async fn emit_status_changed(
        conn: &Connection,
        status: &str,
    ) -> Result<()> {
        let iface_ref = conn
            .object_server()
            .interface::<_, DictationInterface>("/ai/voxctrl/Dictation")
            .await?;
        DictationInterface::status_changed(iface_ref.signal_context(), status).await?;
        Ok(())
    }

    pub async fn emit_text_injected(conn: &Connection, text: &str) -> Result<()> {
        let iface_ref = conn
            .object_server()
            .interface::<_, DictationInterface>("/ai/voxctrl/Dictation")
            .await?;
        DictationInterface::text_injected(iface_ref.signal_context(), text).await?;
        Ok(())
    }

    pub async fn emit_update_available(conn: &Connection, version: &str) -> Result<()> {
        let iface_ref = conn
            .object_server()
            .interface::<_, DictationInterface>("/ai/voxctrl/Dictation")
            .await?;
        DictationInterface::update_available(iface_ref.signal_context(), version).await?;
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use zbus::export::futures_util::StreamExt;
        use zbus::fdo::DBusProxy;

        /// `CheckForUpdate` hands the question to the app over the channel and
        /// returns the app's answer: the new version, or `""` when up to date.
        #[tokio::test]
        async fn check_for_update_returns_the_apps_answer() {
            let state = Arc::new(Mutex::new(AppState::default()));
            let (start_tx, _start_rx) = tokio::sync::mpsc::channel(4);
            let (stop_tx, _stop_rx) = tokio::sync::mpsc::channel(4);
            let (update_tx, mut update_rx) = tokio::sync::mpsc::channel::<UpdateReply>(4);

            let Ok(_conn) = start_service(state, start_tx, stop_tx, update_tx).await else {
                return;
            };
            tokio::spawn(async move {
                while let Some(reply) = update_rx.recv().await {
                    let _ = reply.send(Ok("0.9.0".to_string()));
                }
            });

            let probe = Connection::session().await.unwrap();
            let answer: String = probe
                .call_method(
                    Some("ai.voxctrl.Dictation"),
                    "/ai/voxctrl/Dictation",
                    Some("ai.voxctrl.Dictation"),
                    "CheckForUpdate",
                    &(),
                )
                .await
                .expect("CheckForUpdate is published under its PascalCase name")
                .body()
                .deserialize()
                .unwrap();
            assert_eq!(answer, "0.9.0");
        }

        /// The caller has to keep the connection `start_service` returns.
        ///
        /// zbus closes a connection when its last handle drops, and closing it
        /// releases the requested name and tears down the object server — so a
        /// caller that discards the return value leaves `ai.voxctrl.Dictation`
        /// unowned while startup has already logged it as registered. That is
        /// how the documented start/stop API, and every Cinnamon/MATE shortcut
        /// (which can only reach VoxCtrl by calling `ToggleBinding` on this
        /// name), came to be missing from the bus with nothing in the log.
        #[tokio::test]
        async fn the_bus_name_lives_exactly_as_long_as_the_returned_connection() {
            let state = Arc::new(Mutex::new(AppState::default()));
            let (start_tx, _start_rx) = tokio::sync::mpsc::channel(4);
            let (stop_tx, _stop_rx) = tokio::sync::mpsc::channel(4);
            let (update_tx, _update_rx) = tokio::sync::mpsc::channel(4);

            // No session bus in this environment (plain CI container): there is
            // no bus to make an assertion about.
            let Ok(conn) = start_service(state, start_tx, stop_tx, update_tx).await else {
                return;
            };

            let probe = Connection::session().await.unwrap();
            let dbus = DBusProxy::new(&probe).await.unwrap();
            let name = "ai.voxctrl.Dictation";

            assert!(
                dbus.name_has_owner(name.try_into().unwrap()).await.unwrap(),
                "the connection handed back by `start_service` must own the name"
            );

            // Subscribed before the drop: the release is asynchronous, and
            // polling for it would either race or need an arbitrary sleep.
            let mut lost = dbus
                .receive_name_owner_changed_with_args(&[(0, name), (2, "")])
                .await
                .unwrap();

            drop(conn);

            lost.next().await.unwrap();
            assert!(
                !dbus.name_has_owner(name.try_into().unwrap()).await.unwrap(),
                "dropping the last handle releases the name — this is why it has \
                 to be held for the life of the process"
            );
        }
    }
}

#[cfg(target_os = "linux")]
pub use linux::{emit_status_changed, emit_text_injected, emit_update_available, start_service};

// ── Stub for non-Linux platforms ──────────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
pub async fn start_service(
    _state: Arc<Mutex<AppState>>,
    _start_tx: tokio::sync::mpsc::Sender<String>,
    _stop_tx: tokio::sync::mpsc::Sender<()>,
    _update_tx: tokio::sync::mpsc::Sender<UpdateReply>,
) -> Result<()> {
    tracing::warn!("DBus service not available on this platform");
    Ok(())
}
