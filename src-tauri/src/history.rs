//! The dictation history the app keeps: recent transcripts, in memory, with
//! an opt-in encrypted copy on disk.
//!
//! What this protects against, and what it can't:
//!
//! - **Other programs reading history off the disk.** By default nothing is
//!   written at all. With "Keep history between restarts" on, the file is
//!   XChaCha20-Poly1305 encrypted, owner-only, and its key lives in the
//!   system keyring (or Windows data protection), never beside it. Without a
//!   working keyring, persisting is refused rather than done unprotected.
//! - **Logs.** Transcript text is never logged from here.
//! - **MCP clients.** Reading history over MCP is a separate opt-in.
//!
//! A program already running as the same user can still read VoxCtrl's
//! memory or ask an unlocked keyring for the key; no desktop app can stop
//! that. The docs say so plainly.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Serialize;
use voxctrl_config::HistoryConfig;
use voxctrl_history::keys::KeyStore;
use voxctrl_history::store::{self, Key};
use voxctrl_history::{Entry, History, Limits};

/// What the History tab shows about where history is kept.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HistoryStatus {
    pub enabled: bool,
    /// History is being written to disk, encrypted, right now.
    pub persisting: bool,
    /// Why keeping history between restarts was refused, if it was.
    pub persist_error: Option<String>,
    /// Where the key lives ("the system keyring").
    pub key_store: String,
    pub count: usize,
}

pub struct HistoryService {
    inner: Mutex<Inner>,
    path: PathBuf,
    keys: Box<dyn KeyStore>,
}

struct Inner {
    history: History,
    cfg: HistoryConfig,
    /// Set while persisting: the key the file is encrypted with.
    key: Option<Key>,
    persist_error: Option<String>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn limits(cfg: &HistoryConfig) -> Limits {
    Limits {
        max_entries: cfg.max_entries.min(voxctrl_config::HISTORY_MAX_ENTRIES) as usize,
        max_age_secs: i64::from(cfg.max_age_hours) * 3600,
    }
}

impl HistoryService {
    /// The history file's default location.
    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("voxctrl")
            .join("history")
            .join("history.bin")
    }

    /// A service with nothing loaded yet: an empty, in-memory history. Call
    /// [`Self::apply_config`] to load a persisted one, off the main thread
    /// (the keyring may show an unlock prompt).
    pub fn new(path: PathBuf, keys: Box<dyn KeyStore>) -> Self {
        let cfg = HistoryConfig { persist: false, ..HistoryConfig::default() };
        Self {
            inner: Mutex::new(Inner {
                history: History::new(limits(&cfg)),
                cfg,
                key: None,
                persist_error: None,
            }),
            path,
            keys,
        }
    }

    /// The platform's service: the default path and key store.
    pub fn system() -> Self {
        let path = Self::default_path();
        let dir = path.parent().map(PathBuf::from).unwrap_or_default();
        Self::new(path, voxctrl_history::keys::system_key_store(&dir))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Bring the history in line with `cfg`, and return the config as it
    /// actually took effect: `persist` comes back `false` when keeping
    /// history on disk was refused (no keyring, or its prompt dismissed).
    ///
    /// Blocking: it may talk to the keyring.
    pub fn apply_config(&self, cfg: &HistoryConfig) -> HistoryConfig {
        let mut effective = cfg.clone();
        let mut inner = self.lock();
        let was_persisting = inner.key.is_some();
        inner.history.set_limits(limits(cfg), now());

        if !cfg.enabled {
            inner.history.clear();
        }

        let want_persist = cfg.enabled && cfg.persist;
        if want_persist && !was_persisting {
            match self.start_persisting() {
                Ok((key, loaded)) => {
                    // Keep what was dictated this session, plus what was on disk.
                    let mut all = loaded;
                    all.extend(inner.history.entries().iter().cloned());
                    all.sort_by_key(|e| e.at);
                    for (i, e) in all.iter_mut().enumerate() {
                        e.id = i as u64 + 1;
                    }
                    inner.history = History::from_entries(all, limits(cfg), now());
                    inner.key = Some(key);
                    inner.persist_error = None;
                }
                Err(e) => {
                    tracing::warn!("History stays in memory only: {e:#}");
                    inner.persist_error = Some(format!("{e:#}"));
                    effective.persist = false;
                }
            }
        } else if !want_persist {
            // Not persisting: no file may be left behind (also one from a
            // run that ended before it could clean up), and turning it off
            // removes the key too.
            inner.key = None;
            if let Err(e) = store::delete(&self.path) {
                tracing::warn!("Could not delete the history file: {e:#}");
            }
            if was_persisting {
                if let Err(e) = self.keys.delete() {
                    tracing::warn!("Could not remove the history key from {}: {e:#}", self.keys.describe());
                }
            }
            if !cfg.persist {
                inner.persist_error = None;
            }
        }

        inner.cfg = effective.clone();
        self.save_locked(&mut inner);
        effective
    }

    /// Get (or create) the key and read whatever was saved with it.
    fn start_persisting(&self) -> anyhow::Result<(Key, Vec<Entry>)> {
        let key = self.keys.load_or_create()?;
        let loaded = match store::load(&self.path, &key) {
            Ok(entries) => entries,
            Err(e) => {
                // Written with a key the keyring no longer has, or damaged.
                // It can't be read by anyone now; start afresh.
                tracing::warn!("Discarding an unreadable history file: {e:#}");
                store::delete(&self.path)?;
                Vec::new()
            }
        };
        Ok((key, loaded))
    }

    fn save_locked(&self, inner: &mut Inner) {
        let Some(key) = inner.key else { return };
        if let Err(e) = store::save(&self.path, &key, inner.history.entries()) {
            tracing::warn!("Could not save the history: {e:#}");
        }
    }

    /// Record a delivered dictation. Returns its id, or `None` when history
    /// is off or the target opted out (the caller checks the latter).
    pub fn record(&self, target_id: &str, target_label: &str, text: &str) -> Option<u64> {
        let mut inner = self.lock();
        if !inner.cfg.enabled {
            return None;
        }
        let id = inner.history.add(now(), target_id, target_label, text);
        if id.is_some() {
            self.save_locked(&mut inner);
        }
        id
    }

    /// Drop expired entries (and save, if any went).
    fn prune_locked(&self, inner: &mut Inner) {
        if inner.history.prune(now()) {
            self.save_locked(inner);
        }
    }

    /// Newest first, filtered by `query`.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Entry> {
        let mut inner = self.lock();
        self.prune_locked(&mut inner);
        inner.history.search(query, limit)
    }

    pub fn get(&self, id: u64) -> Option<Entry> {
        let mut inner = self.lock();
        self.prune_locked(&mut inner);
        inner.history.get(id).cloned()
    }

    pub fn delete(&self, id: u64) -> bool {
        let mut inner = self.lock();
        let removed = inner.history.delete(id);
        if removed {
            self.save_locked(&mut inner);
        }
        removed
    }

    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.history.clear();
        self.save_locked(&mut inner);
    }

    /// Whether MCP clients may read the history.
    pub fn mcp_access(&self) -> bool {
        let inner = self.lock();
        inner.cfg.enabled && inner.cfg.mcp_access
    }

    pub fn status(&self) -> HistoryStatus {
        let mut inner = self.lock();
        self.prune_locked(&mut inner);
        HistoryStatus {
            enabled: inner.cfg.enabled,
            persisting: inner.key.is_some(),
            persist_error: inner.persist_error.clone(),
            key_store: self.keys.describe().to_string(),
            count: inner.history.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use voxctrl_history::keys::MemoryKeyStore;

    /// A key store that always refuses, like a desktop with no keyring.
    struct NoKeyring;
    impl KeyStore for NoKeyring {
        fn load(&self) -> anyhow::Result<Option<Key>> {
            anyhow::bail!("no Secret Service on the session bus")
        }
        fn save(&self, _: &Key) -> anyhow::Result<()> {
            anyhow::bail!("no Secret Service on the session bus")
        }
        fn delete(&self) -> anyhow::Result<()> {
            Ok(())
        }
        fn describe(&self) -> &'static str {
            "no keyring"
        }
    }

    /// Shares one key between services, as the keyring would across restarts.
    #[derive(Clone, Default)]
    struct SharedKeys(std::sync::Arc<MemoryKeyStore>);
    impl KeyStore for SharedKeys {
        fn load(&self) -> anyhow::Result<Option<Key>> {
            self.0.load()
        }
        fn save(&self, k: &Key) -> anyhow::Result<()> {
            self.0.save(k)
        }
        fn delete(&self) -> anyhow::Result<()> {
            self.0.delete()
        }
        fn describe(&self) -> &'static str {
            "test keyring"
        }
    }

    fn cfg(persist: bool) -> HistoryConfig {
        HistoryConfig { persist, ..HistoryConfig::default() }
    }

    #[test]
    fn by_default_nothing_touches_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history").join("history.bin");
        let svc = HistoryService::new(path.clone(), Box::new(SharedKeys::default()));
        svc.apply_config(&cfg(false));
        svc.record("default", "Focused Window", "something private");
        assert_eq!(svc.search("", 10).len(), 1);
        assert!(!path.exists());
        assert!(!dir.path().join("history").exists());
        assert!(!svc.status().persisting);
    }

    #[test]
    fn persisted_history_survives_a_restart_encrypted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.bin");
        let keys = SharedKeys::default();

        let first = HistoryService::new(path.clone(), Box::new(keys.clone()));
        assert!(first.apply_config(&cfg(true)).persist);
        first.record("default", "Focused Window", "my bank PIN is 4321");
        assert!(path.exists());
        let raw = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains("4321"));

        let second = HistoryService::new(path.clone(), Box::new(keys));
        second.apply_config(&cfg(true));
        let entries = second.search("", 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text, "my bank PIN is 4321");
    }

    #[test]
    fn without_a_keyring_persisting_is_refused_not_done_in_plain_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.bin");
        let svc = HistoryService::new(path.clone(), Box::new(NoKeyring));
        let effective = svc.apply_config(&cfg(true));
        assert!(!effective.persist, "the setting is turned back off");
        svc.record("default", "Focused Window", "still kept in memory");
        assert!(!path.exists());
        let status = svc.status();
        assert!(!status.persisting);
        assert!(status.persist_error.unwrap().contains("Secret Service"));
        assert_eq!(status.count, 1);
    }

    #[test]
    fn turning_persistence_off_deletes_the_file_and_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.bin");
        let keys = SharedKeys::default();
        let svc = HistoryService::new(path.clone(), Box::new(keys.clone()));
        svc.apply_config(&cfg(true));
        svc.record("default", "Focused Window", "hello");
        assert!(path.exists());
        assert!(keys.load().unwrap().is_some());

        svc.apply_config(&cfg(false));
        assert!(!path.exists());
        assert!(keys.load().unwrap().is_none());
        assert_eq!(svc.search("", 10).len(), 1, "this session's history stays in memory");
    }

    #[test]
    fn turning_history_off_clears_it_and_stops_recording() {
        let dir = tempfile::tempdir().unwrap();
        let svc = HistoryService::new(dir.path().join("h.bin"), Box::new(SharedKeys::default()));
        svc.apply_config(&cfg(false));
        svc.record("default", "Focused Window", "one");
        svc.apply_config(&HistoryConfig { enabled: false, ..cfg(false) });
        assert_eq!(svc.search("", 10).len(), 0);
        assert_eq!(svc.record("default", "Focused Window", "two"), None);
        assert_eq!(svc.search("", 10).len(), 0);
    }

    #[test]
    fn a_file_from_a_lost_key_is_discarded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.bin");
        let old = SharedKeys::default();
        let svc = HistoryService::new(path.clone(), Box::new(old));
        svc.apply_config(&cfg(true));
        svc.record("default", "Focused Window", "old");

        // The keyring was reset: a new key can't read the old file.
        let svc2 = HistoryService::new(path.clone(), Box::new(SharedKeys::default()));
        assert!(svc2.apply_config(&cfg(true)).persist);
        assert!(svc2.search("", 10).is_empty());
    }

    #[test]
    fn delete_and_clear_reach_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.bin");
        let keys = SharedKeys::default();
        let svc = HistoryService::new(path.clone(), Box::new(keys.clone()));
        svc.apply_config(&cfg(true));
        let a = svc.record("default", "Focused Window", "first").unwrap();
        svc.record("default", "Focused Window", "second");
        assert!(svc.delete(a));

        let reloaded = HistoryService::new(path.clone(), Box::new(keys.clone()));
        reloaded.apply_config(&cfg(true));
        let texts: Vec<_> = reloaded.search("", 10).into_iter().map(|e| e.text).collect();
        assert_eq!(texts, vec!["second"]);

        reloaded.clear();
        let again = HistoryService::new(path, Box::new(keys));
        again.apply_config(&cfg(true));
        assert!(again.search("", 10).is_empty());
    }

    #[test]
    fn mcp_access_needs_both_switches() {
        let dir = tempfile::tempdir().unwrap();
        let svc = HistoryService::new(dir.path().join("h.bin"), Box::new(SharedKeys::default()));
        svc.apply_config(&cfg(false));
        assert!(!svc.mcp_access());
        svc.apply_config(&HistoryConfig { mcp_access: true, ..cfg(false) });
        assert!(svc.mcp_access());
        svc.apply_config(&HistoryConfig { mcp_access: true, enabled: false, ..cfg(false) });
        assert!(!svc.mcp_access());
    }
}
