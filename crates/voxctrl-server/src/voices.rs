//! Voices that clients upload: a reference clip, and for engines that need
//! one, its transcript.
//!
//! Each client's voices are kept in a folder of their own and can only be
//! listed, used or removed by a request that carries the same client id. The
//! id is not a password (every client already holds the server's API key);
//! it keeps one person's voices out of everyone else's voice picker and out
//! of their requests.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Most voices one client may keep on the server.
pub const MAX_VOICES_PER_CLIENT: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct StoredVoice {
    pub name: String,
    /// Whatever the client sent to recognise this version of the voice by.
    /// The server only stores it and hands it back.
    pub fingerprint: String,
}

#[derive(Debug, Clone)]
pub struct VoiceStore {
    root: PathBuf,
}

/// A client id as a folder name: 16 to 64 hex digits, nothing else.
pub fn valid_client_id(id: &str) -> bool {
    (16..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit())
}

/// A voice name as a file stem. No dots or separators, so it cannot name
/// anything outside the client's folder.
pub fn valid_voice_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' '))
        && name.trim() == name
}

impl VoiceStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// `<data>/voxctrl/client-voices`, beside the server's models.
    pub fn default_root() -> PathBuf {
        dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("voxctrl").join("client-voices")
    }

    /// The folder holding one client's voices. `None` for an id that is not
    /// a valid one; the folder may not exist yet.
    pub fn client_dir(&self, client: &str) -> Option<PathBuf> {
        valid_client_id(client).then(|| self.root.join(client.to_ascii_lowercase()))
    }

    pub fn list(&self, client: &str) -> Vec<StoredVoice> {
        let Some(dir) = self.client_dir(client) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
        let mut voices: Vec<StoredVoice> = entries
            .flatten()
            .filter_map(|e| {
                let path = e.path();
                (path.extension().and_then(|x| x.to_str()) == Some("wav")).then_some(path)
            })
            .filter_map(|wav| {
                let name = wav.file_stem()?.to_str()?.to_string();
                let fingerprint =
                    std::fs::read_to_string(wav.with_extension("fp")).unwrap_or_default().trim().to_string();
                Some(StoredVoice { name, fingerprint })
            })
            .collect();
        voices.sort_by(|a, b| a.name.cmp(&b.name));
        voices
    }

    pub fn has(&self, client: &str, name: &str) -> bool {
        valid_voice_name(name) && self.wav_path(client, name).is_some_and(|p| p.is_file())
    }

    fn wav_path(&self, client: &str, name: &str) -> Option<PathBuf> {
        Some(self.client_dir(client)?.join(format!("{name}.wav")))
    }

    /// Store or replace a voice. The caller has validated the id and name.
    pub fn put(&self, client: &str, name: &str, wav: &[u8], transcript: Option<&str>, fingerprint: &str) -> Result<()> {
        let dir = self.client_dir(client).context("invalid client id")?;
        anyhow::ensure!(valid_voice_name(name), "invalid voice name");
        std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let wav_path = dir.join(format!("{name}.wav"));
        // Written whole under another name first, so a request never reads
        // half a clip.
        let partial = dir.join(format!("{name}.wav.part"));
        std::fs::write(&partial, wav).context("write the voice clip")?;
        std::fs::rename(&partial, &wav_path).context("store the voice clip")?;
        write_or_remove(&wav_path.with_extension("txt"), transcript.map(str::trim).filter(|t| !t.is_empty()))?;
        write_or_remove(&wav_path.with_extension("fp"), Some(fingerprint.trim()))?;
        Ok(())
    }

    /// Remove a voice. `Ok(false)` when there was none.
    pub fn delete(&self, client: &str, name: &str) -> Result<bool> {
        if !valid_voice_name(name) {
            return Ok(false);
        }
        let Some(wav) = self.wav_path(client, name) else { return Ok(false) };
        if !wav.is_file() {
            return Ok(false);
        }
        std::fs::remove_file(&wav).context("remove the voice clip")?;
        let _ = std::fs::remove_file(wav.with_extension("txt"));
        let _ = std::fs::remove_file(wav.with_extension("fp"));
        Ok(true)
    }
}

fn write_or_remove(path: &Path, content: Option<&str>) -> Result<()> {
    match content {
        Some(content) => std::fs::write(path, content).with_context(|| format!("write {}", path.display())),
        None => {
            let _ = std::fs::remove_file(path);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALICE: &str = "0123456789abcdef0123456789abcdef";
    const BOB: &str = "fedcba9876543210fedcba9876543210";

    fn store() -> (tempfile::TempDir, VoiceStore) {
        let tmp = tempfile::tempdir().unwrap();
        let store = VoiceStore::new(tmp.path().join("client-voices"));
        (tmp, store)
    }

    #[test]
    fn a_voice_is_stored_with_its_transcript_and_fingerprint() {
        let (_tmp, store) = store();
        store.put(ALICE, "Rick", b"RIFFdata", Some("  hello there \n"), "abc123").unwrap();
        assert_eq!(store.list(ALICE), vec![StoredVoice { name: "Rick".into(), fingerprint: "abc123".into() }]);
        assert!(store.has(ALICE, "Rick"));
        let dir = store.client_dir(ALICE).unwrap();
        assert_eq!(std::fs::read(dir.join("Rick.wav")).unwrap(), b"RIFFdata");
        assert_eq!(std::fs::read_to_string(dir.join("Rick.txt")).unwrap(), "hello there");
    }

    #[test]
    fn replacing_a_voice_without_a_transcript_drops_the_old_one() {
        let (_tmp, store) = store();
        store.put(ALICE, "Rick", b"RIFFone", Some("old words"), "v1").unwrap();
        store.put(ALICE, "Rick", b"RIFFtwo", None, "v2").unwrap();
        let dir = store.client_dir(ALICE).unwrap();
        assert!(!dir.join("Rick.txt").exists());
        assert_eq!(store.list(ALICE)[0].fingerprint, "v2");
        assert_eq!(std::fs::read(dir.join("Rick.wav")).unwrap(), b"RIFFtwo");
    }

    #[test]
    fn one_clients_voices_are_invisible_to_another() {
        let (_tmp, store) = store();
        store.put(ALICE, "Rick", b"RIFF", None, "v1").unwrap();
        assert!(store.list(BOB).is_empty());
        assert!(!store.has(BOB, "Rick"));
        assert!(!store.delete(BOB, "Rick").unwrap(), "another client cannot remove it");
        assert!(store.has(ALICE, "Rick"));
    }

    #[test]
    fn deleting_removes_every_file_of_the_voice() {
        let (_tmp, store) = store();
        store.put(ALICE, "Rick", b"RIFF", Some("words"), "v1").unwrap();
        assert!(store.delete(ALICE, "Rick").unwrap());
        assert!(store.list(ALICE).is_empty());
        assert_eq!(std::fs::read_dir(store.client_dir(ALICE).unwrap()).unwrap().count(), 0);
        assert!(!store.delete(ALICE, "Rick").unwrap());
    }

    #[test]
    fn ids_and_names_that_could_leave_the_folder_are_refused() {
        for id in ["", "short", "../../../etc", "0123456789abcdef/..", "0123456789abcdeg", &"a".repeat(65)] {
            assert!(!valid_client_id(id), "{id}");
        }
        assert!(valid_client_id(ALICE));
        for name in ["", "..", "a/b", "a\\b", "Rick.wav", " Rick", "Rick ", "a\nb", &"n".repeat(65)] {
            assert!(!valid_voice_name(name), "{name:?}");
        }
        for name in ["Rick", "my voice", "voice_2", "Deep-Voice"] {
            assert!(valid_voice_name(name), "{name}");
        }
        let (_tmp, store) = store();
        assert!(store.put("../x", "Rick", b"RIFF", None, "v").is_err());
        assert!(store.put(ALICE, "../Rick", b"RIFF", None, "v").is_err());
        assert!(store.client_dir("nope").is_none());
    }
}
