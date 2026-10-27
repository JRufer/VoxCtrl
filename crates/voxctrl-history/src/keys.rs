//! Where the history's encryption key lives: never in a file beside the
//! history, but with the operating system's own secret storage.
//!
//! - **Linux:** the desktop keyring through the freedesktop Secret Service
//!   API (KWallet on KDE Plasma, GNOME Keyring elsewhere), as an item
//!   labelled "VoxCtrl dictation history key". If the keyring is locked, the
//!   desktop's own unlock prompt is shown.
//! - **Windows:** a random key protected with DPAPI, which only this Windows
//!   user account can unprotect, kept in `history.key` beside the history.
//!
//! With neither available, [`KeyStore::load_or_create`] fails and the caller
//! keeps history in memory only, rather than writing it unprotected.

use anyhow::Result;

use crate::store::{new_key, Key};

/// Somewhere a history key can be kept.
pub trait KeyStore: Send + Sync {
    /// The stored key, or `None` if there is none yet.
    fn load(&self) -> Result<Option<Key>>;
    /// Store `key`, replacing any earlier one.
    fn save(&self, key: &Key) -> Result<()>;
    /// Remove the stored key, if any.
    fn delete(&self) -> Result<()>;
    /// A short description for messages ("the system keyring").
    fn describe(&self) -> &'static str;

    /// The stored key, creating and storing a new one the first time.
    fn load_or_create(&self) -> Result<Key> {
        if let Some(key) = self.load()? {
            return Ok(key);
        }
        let key = new_key();
        self.save(&key)?;
        Ok(key)
    }
}

/// A key kept only in memory, for tests.
#[derive(Default)]
pub struct MemoryKeyStore(std::sync::Mutex<Option<Key>>);

impl KeyStore for MemoryKeyStore {
    fn load(&self) -> Result<Option<Key>> {
        Ok(*self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
    fn save(&self, key: &Key) -> Result<()> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(*key);
        Ok(())
    }
    fn delete(&self) -> Result<()> {
        *self.0.lock().unwrap_or_else(|e| e.into_inner()) = None;
        Ok(())
    }
    fn describe(&self) -> &'static str {
        "memory"
    }
}

/// The platform's key store.
pub fn system_key_store(_history_dir: &std::path::Path) -> Box<dyn KeyStore> {
    #[cfg(target_os = "linux")]
    {
        Box::new(secret_service::SecretServiceKeyStore)
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(dpapi::DpapiKeyStore::new(_history_dir.join("history.key")))
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Box::new(Unavailable)
    }
}

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
struct Unavailable;

#[cfg(not(any(target_os = "linux", target_os = "windows")))]
impl KeyStore for Unavailable {
    fn load(&self) -> Result<Option<Key>> {
        anyhow::bail!("no secure key storage on this platform")
    }
    fn save(&self, _key: &Key) -> Result<()> {
        anyhow::bail!("no secure key storage on this platform")
    }
    fn delete(&self) -> Result<()> {
        Ok(())
    }
    fn describe(&self) -> &'static str {
        "no key storage"
    }
}

// ── Linux: freedesktop Secret Service ────────────────────────────────────────

#[cfg(target_os = "linux")]
pub mod secret_service {
    //! A minimal Secret Service client: just enough to keep one key.
    //!
    //! The "plain" session algorithm is used: the key crosses the user's own
    //! session bus unencrypted, as with most Secret Service clients. That bus
    //! is private to the user, and anything that could read it could equally
    //! ask the keyring for the key directly.

    use std::collections::HashMap;

    use anyhow::{anyhow, bail, Context, Result};
    use zbus::blocking::{Connection, Proxy};
    use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

    use super::KeyStore;
    use crate::store::Key;

    const DEST: &str = "org.freedesktop.secrets";
    const SERVICE_PATH: &str = "/org/freedesktop/secrets";
    const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
    const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
    const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
    const PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";
    const LABEL: &str = "VoxCtrl dictation history key";

    /// (session, parameters, value, content type)
    type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

    pub struct SecretServiceKeyStore;

    fn attributes() -> HashMap<&'static str, &'static str> {
        HashMap::from([
            ("application", "voxctrl"),
            ("purpose", "dictation-history-key"),
        ])
    }

    struct Session {
        conn: Connection,
        service: Proxy<'static>,
        session: OwnedObjectPath,
    }

    impl Session {
        fn open() -> Result<Self> {
            let conn = Connection::session().context("no D-Bus session bus")?;
            let service = Proxy::new(&conn, DEST, SERVICE_PATH, SERVICE_IFACE)
                .context("no Secret Service on the session bus")?;
            let (_, session): (OwnedValue, OwnedObjectPath) = service
                .call("OpenSession", &("plain", Value::from("")))
                .context(
                    "the keyring didn't open a session (is KWallet or GNOME Keyring running?)",
                )?;
            Ok(Self {
                conn,
                service,
                session,
            })
        }

        /// Run a prompt (unlocking the keyring, usually) and wait for it,
        /// returning the prompt's result. `None` when no prompt was needed.
        fn prompt(&self, path: &OwnedObjectPath) -> Result<Option<OwnedValue>> {
            if path.as_str() == "/" {
                return Ok(None);
            }
            let prompt = Proxy::new(&self.conn, DEST, path.as_str(), PROMPT_IFACE)?;
            let mut completed = prompt.receive_signal("Completed")?;
            let _: () = prompt.call("Prompt", &(""))?;
            let msg = completed
                .next()
                .ok_or_else(|| anyhow!("the keyring prompt went away"))?;
            let (dismissed, result): (bool, OwnedValue) = msg.body().deserialize()?;
            if dismissed {
                bail!("the keyring prompt was dismissed");
            }
            Ok(Some(result))
        }

        /// The collection new items go in: whatever the keyring's "default"
        /// alias points at. A keyring that has none yet (a fresh account, or
        /// one whose login keyring was deleted) gets one, through the
        /// desktop's own "choose a password for the new keyring" prompt.
        fn default_collection(&self) -> Result<OwnedObjectPath> {
            let alias: OwnedObjectPath = self.service.call("ReadAlias", &("default"))?;
            let existing: Vec<OwnedObjectPath> =
                self.service.get_property("Collections").unwrap_or_default();
            if alias.as_str() != "/" && existing.iter().any(|c| c == &alias) {
                return Ok(alias);
            }
            let mut props: HashMap<&str, Value> = HashMap::new();
            props.insert(
                "org.freedesktop.Secret.Collection.Label",
                Value::from("Login"),
            );
            let (collection, prompt): (OwnedObjectPath, OwnedObjectPath) = self
                .service
                .call("CreateCollection", &(&props, "default"))
                .context("the keyring has no default collection and couldn't create one")?;
            if collection.as_str() != "/" {
                return Ok(collection);
            }
            let created = self
                .prompt(&prompt)?
                .ok_or_else(|| anyhow!("the keyring didn't create a collection"))?;
            OwnedObjectPath::try_from(created)
                .map_err(|_| anyhow!("the keyring returned no collection"))
        }

        fn find(&self) -> Result<Option<OwnedObjectPath>> {
            let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
                self.service.call("SearchItems", &(attributes()))?;
            if let Some(item) = unlocked.into_iter().next() {
                return Ok(Some(item));
            }
            let Some(item) = locked.into_iter().next() else {
                return Ok(None);
            };
            let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
                self.service.call("Unlock", &(vec![item.clone()]))?;
            self.prompt(&prompt)?;
            Ok(Some(item))
        }
    }

    impl KeyStore for SecretServiceKeyStore {
        fn load(&self) -> Result<Option<Key>> {
            let s = Session::open()?;
            let Some(item) = s.find()? else {
                return Ok(None);
            };
            let proxy = Proxy::new(&s.conn, DEST, item.as_str(), ITEM_IFACE)?;
            let (_, _, value, _): Secret = proxy.call("GetSecret", &(&s.session))?;
            let key: Key = value
                .as_slice()
                .try_into()
                .map_err(|_| anyhow!("the keyring's history key has the wrong length"))?;
            Ok(Some(key))
        }

        fn save(&self, key: &Key) -> Result<()> {
            let s = Session::open()?;
            let path = s.default_collection()?;
            let collection = Proxy::new(&s.conn, DEST, path.as_str(), COLLECTION_IFACE)?;
            let mut props: HashMap<&str, Value> = HashMap::new();
            props.insert("org.freedesktop.Secret.Item.Label", Value::from(LABEL));
            props.insert(
                "org.freedesktop.Secret.Item.Attributes",
                Value::from(attributes()),
            );
            let secret = (
                ObjectPath::try_from(s.session.as_str())?,
                Vec::<u8>::new(),
                key.to_vec(),
                "application/octet-stream",
            );
            let call = |c: &Proxy| -> zbus::Result<(OwnedObjectPath, OwnedObjectPath)> {
                c.call("CreateItem", &(&props, &secret, true))
            };
            let (item, prompt) = match call(&collection) {
                Ok(r) => r,
                Err(_) => {
                    // A locked default collection: unlock it, then try again.
                    let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) =
                        s.service.call("Unlock", &(vec![&path]))?;
                    s.prompt(&prompt)?;
                    call(&collection).context("the keyring refused to store the history key")?
                }
            };
            if item.as_str() == "/" {
                s.prompt(&prompt)?;
            }
            Ok(())
        }

        fn delete(&self) -> Result<()> {
            let s = Session::open()?;
            let (unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) =
                s.service.call("SearchItems", &(attributes()))?;
            for item in unlocked.into_iter().chain(locked) {
                let proxy = Proxy::new(&s.conn, DEST, item.as_str(), ITEM_IFACE)?;
                let prompt: OwnedObjectPath = proxy.call("Delete", &())?;
                s.prompt(&prompt)?;
            }
            Ok(())
        }

        fn describe(&self) -> &'static str {
            "the system keyring"
        }
    }
}

// ── Windows: DPAPI ───────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
pub mod dpapi {
    use std::path::PathBuf;

    use anyhow::{anyhow, bail, Result};
    use windows_sys::Win32::Foundation::LocalFree;
    use windows_sys::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    use super::KeyStore;
    use crate::store::Key;

    /// A random key, protected with DPAPI for the current Windows user and
    /// kept in a file; only that user account can turn it back into the key.
    pub struct DpapiKeyStore {
        path: PathBuf,
    }

    impl DpapiKeyStore {
        pub fn new(path: PathBuf) -> Self {
            Self { path }
        }
    }

    fn transform(data: &[u8], protect: bool) -> Result<Vec<u8>> {
        let input = CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };
        // SAFETY: input points to `data` for the call; output is allocated by
        // the API and released with LocalFree below.
        let ok = unsafe {
            if protect {
                CryptProtectData(
                    &input,
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            } else {
                CryptUnprotectData(
                    &input,
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    std::ptr::null(),
                    std::ptr::null(),
                    CRYPTPROTECT_UI_FORBIDDEN,
                    &mut output,
                )
            }
        };
        if ok == 0 || output.pbData.is_null() {
            bail!("Windows data protection failed");
        }
        // SAFETY: the API returned `cbData` bytes at `pbData`.
        let out =
            unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
        unsafe { LocalFree(output.pbData as _) };
        Ok(out)
    }

    impl KeyStore for DpapiKeyStore {
        fn load(&self) -> Result<Option<Key>> {
            let protected = match std::fs::read(&self.path) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e.into()),
            };
            let key = transform(&protected, false)?;
            Ok(Some(key.as_slice().try_into().map_err(|_| {
                anyhow!("the history key has the wrong length")
            })?))
        }

        fn save(&self, key: &Key) -> Result<()> {
            if let Some(dir) = self.path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            std::fs::write(&self.path, transform(key, true)?)?;
            Ok(())
        }

        fn delete(&self) -> Result<()> {
            match std::fs::remove_file(&self.path) {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.into()),
            }
        }

        fn describe(&self) -> &'static str {
            "Windows data protection"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_created_once_and_then_reused() {
        let ks = MemoryKeyStore::default();
        let first = ks.load_or_create().unwrap();
        assert_eq!(ks.load_or_create().unwrap(), first);
        ks.delete().unwrap();
        assert_ne!(ks.load_or_create().unwrap(), first);
    }

    /// Against a real Secret Service. Run with a keyring daemon on the session
    /// bus, e.g. (with a fresh `HOME`, and a non-empty password so a login
    /// keyring is created) `dbus-run-session -- sh -c 'echo -n test |
    /// gnome-keyring-daemon --unlock --components=secrets >/dev/null; cargo
    /// test -p voxctrl-history -- --ignored'`.
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "needs a Secret Service daemon on the session bus"]
    fn secret_service_round_trip() {
        let ks = secret_service::SecretServiceKeyStore;
        ks.delete().unwrap();
        assert_eq!(ks.load().unwrap(), None);
        let key = ks.load_or_create().unwrap();
        assert_eq!(ks.load().unwrap(), Some(key));
        let other = crate::store::new_key();
        ks.save(&other).unwrap();
        assert_eq!(
            ks.load().unwrap(),
            Some(other),
            "saving replaces the earlier key"
        );
        ks.delete().unwrap();
        assert_eq!(ks.load().unwrap(), None);
    }
}
