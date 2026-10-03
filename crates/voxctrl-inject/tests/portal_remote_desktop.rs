//! The RemoteDesktop portal flow, against a stand-in portal on a private
//! session bus: the client must create a session, select the keyboard, start
//! it (saving the restore token the portal returns), and then send the chord's
//! key events in order. A second run must present the saved token.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use zbus5::zvariant::{ObjectPath, OwnedValue, Value};
use zbus5::{interface, message::Header};

// The `#[interface]` macro writes its paths as `zbus::...`; this crate is
// pulled in under another name (it must not clash with the workspace's zbus 4).
#[allow(unused_imports)]
mod zbus {
    pub use zbus5::*;
}

#[derive(Default)]
struct Record {
    /// (keycode, state) in arrival order.
    keys: Vec<(i32, u32)>,
    /// The restore_token each SelectDevices call carried, if any.
    select_tokens: Vec<Option<String>>,
    starts: usize,
}

struct Portal {
    rec: Arc<Mutex<Record>>,
}

fn sender_part(h: &Header<'_>) -> String {
    h.sender().map(|s| s.as_str().trim_start_matches(':').replace('.', "_")).unwrap_or_default()
}

fn token_of(opts: &HashMap<String, Value<'_>>, key: &str) -> String {
    match opts.get(key) {
        Some(Value::Str(s)) => s.to_string(),
        _ => "t".into(),
    }
}

/// Answer a request the way the real portal does: return the request's
/// object path now, and deliver the outcome as a `Response` signal on it,
/// after the method reply so the client is already listening.
fn respond(conn: &zbus5::Connection, request_path: String, results: HashMap<String, OwnedValue>) {
    let conn = conn.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(60));
        let _ = zbus5::block_on(conn.emit_signal(
            None::<&str>,
            request_path,
            "org.freedesktop.portal.Request",
            "Response",
            &(0u32, results),
        ));
    });
}

#[interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl Portal {
    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        1
    }

    #[zbus(property)]
    fn version(&self) -> u32 {
        2
    }

    async fn create_session(
        &self,
        options: HashMap<String, Value<'_>>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus5::Connection,
    ) -> zbus5::fdo::Result<ObjectPath<'static>> {
        let who = sender_part(&hdr);
        let request = format!(
            "/org/freedesktop/portal/desktop/request/{who}/{}",
            token_of(&options, "handle_token")
        );
        let session = format!(
            "/org/freedesktop/portal/desktop/session/{who}/{}",
            token_of(&options, "session_handle_token")
        );
        let mut results = HashMap::new();
        results.insert("session_handle".to_string(), Value::from(session).try_into().unwrap());
        respond(conn, request.clone(), results);
        Ok(ObjectPath::try_from(request).unwrap())
    }

    async fn select_devices(
        &self,
        _session: ObjectPath<'_>,
        options: HashMap<String, Value<'_>>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus5::Connection,
    ) -> zbus5::fdo::Result<ObjectPath<'static>> {
        let who = sender_part(&hdr);
        let request = format!(
            "/org/freedesktop/portal/desktop/request/{who}/{}",
            token_of(&options, "handle_token")
        );
        let token = options.get("restore_token").and_then(|v| match v {
            Value::Str(s) => Some(s.to_string()),
            _ => None,
        });
        self.rec.lock().unwrap().select_tokens.push(token);
        respond(conn, request.clone(), HashMap::new());
        Ok(ObjectPath::try_from(request).unwrap())
    }

    async fn start(
        &self,
        _session: ObjectPath<'_>,
        _parent: &str,
        options: HashMap<String, Value<'_>>,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &zbus5::Connection,
    ) -> zbus5::fdo::Result<ObjectPath<'static>> {
        let who = sender_part(&hdr);
        let request = format!(
            "/org/freedesktop/portal/desktop/request/{who}/{}",
            token_of(&options, "handle_token")
        );
        self.rec.lock().unwrap().starts += 1;
        let mut results = HashMap::new();
        results.insert("devices".to_string(), Value::from(1u32).try_into().unwrap());
        results.insert("restore_token".to_string(), Value::from("granted-token-1").try_into().unwrap());
        respond(conn, request.clone(), results);
        Ok(ObjectPath::try_from(request).unwrap())
    }

    fn notify_keyboard_keycode(
        &self,
        _session: ObjectPath<'_>,
        _options: HashMap<String, Value<'_>>,
        keycode: i32,
        state: u32,
    ) {
        self.rec.lock().unwrap().keys.push((keycode, state));
    }
}

struct Bus(Child);

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn the_paste_shortcut_goes_through_the_remote_desktop_portal() {
    use std::io::{BufRead, BufReader};
    let Ok(mut child) = Command::new("dbus-daemon")
        .args(["--session", "--nofork", "--print-address=1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        eprintln!("dbus-daemon not available; skipping");
        return;
    };
    let mut address = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut address).unwrap();
    let _bus = Bus(child);
    let address = address.trim().to_string();
    std::env::set_var("DBUS_SESSION_BUS_ADDRESS", &address);

    // The saved grant lives under the config dir; keep it out of the real one.
    let cfg = std::env::temp_dir().join(format!("voxctrl-portal-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cfg);
    std::env::set_var("XDG_CONFIG_HOME", &cfg);

    let rec = Arc::new(Mutex::new(Record::default()));
    let _service = zbus5::connection::Builder::address(address.as_str())
        .unwrap()
        .name("org.freedesktop.portal.Desktop")
        .unwrap()
        .serve_at("/org/freedesktop/portal/desktop", Portal { rec: rec.clone() })
        .unwrap()
        .build()
        .await
        .unwrap();

    voxctrl_inject::portal_send_paste(voxctrl_inject::Shortcut::CtrlV).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    {
        let r = rec.lock().unwrap();
        assert_eq!(r.starts, 1);
        assert_eq!(r.select_tokens, vec![None], "the first run has no saved grant");
        // Ctrl down, V down, V up, Ctrl up (evdev codes).
        assert_eq!(r.keys, vec![(29, 1), (47, 1), (47, 0), (29, 0)]);
    }
    let saved = std::fs::read_to_string(cfg.join("voxctrl").join("portal-keyboard-token")).unwrap();
    assert_eq!(saved.trim(), "granted-token-1", "the portal's restore token was not saved");

    // The session stays open: a second paste needs no new dialog.
    voxctrl_inject::portal_send_paste(voxctrl_inject::Shortcut::CtrlShiftV).await.unwrap();
    let r = rec.lock().unwrap();
    assert_eq!(r.starts, 1, "a second paste must reuse the session");
    assert_eq!(
        &r.keys[4..],
        &[(29, 1), (42, 1), (47, 1), (47, 0), (42, 0), (29, 0)]
    );
}
