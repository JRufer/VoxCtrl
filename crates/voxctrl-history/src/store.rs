//! The opt-in on-disk copy of the history: encrypted, owner-only, atomic.
//!
//! Layout: [`MAGIC`] (also the cipher's associated data), a 24-byte random
//! nonce, then the XChaCha20-Poly1305 encryption of the entries as JSON. A
//! wrong key, a truncated file or any tampering fails authentication, and the
//! file is then treated as unreadable rather than half-read.

use std::io::Write;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use chacha20poly1305::aead::{Aead, AeadCore, KeyInit, OsRng, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

use crate::Entry;

/// Identifies the format; bound into the ciphertext as associated data.
pub const MAGIC: &[u8] = b"VOXCTRL-HISTORY\x01";
const NONCE_LEN: usize = 24;

/// A 256-bit key.
pub type Key = [u8; 32];

/// A fresh random key.
pub fn new_key() -> Key {
    XChaCha20Poly1305::generate_key(&mut OsRng).into()
}

/// Encrypt `entries` into the file format.
pub fn seal(key: &Key, entries: &[Entry]) -> Result<Vec<u8>> {
    let plaintext = serde_json::to_vec(entries)?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = cipher
        .encrypt(
            &nonce,
            Payload {
                msg: &plaintext,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow!("could not encrypt the history"))?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt the file format back into entries.
pub fn open(key: &Key, data: &[u8]) -> Result<Vec<Entry>> {
    if data.len() < MAGIC.len() + NONCE_LEN || !data.starts_with(MAGIC) {
        bail!("not a VoxCtrl history file");
    }
    let nonce = XNonce::from_slice(&data[MAGIC.len()..MAGIC.len() + NONCE_LEN]);
    let ciphertext = &data[MAGIC.len() + NONCE_LEN..];
    let cipher = XChaCha20Poly1305::new(key.into());
    let plaintext = cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad: MAGIC,
            },
        )
        .map_err(|_| anyhow!("the history file can't be decrypted with the key in the keyring"))?;
    Ok(serde_json::from_slice(&plaintext)?)
}

/// Read and decrypt the history at `path`; an absent file is an empty history.
pub fn load(path: &Path, key: &Key) -> Result<Vec<Entry>> {
    match std::fs::read(path) {
        Ok(data) => open(key, &data),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
    }
}

/// Encrypt and write the history to `path`, replacing it atomically. The
/// folder is created owner-only and the file is created owner-only (0700 /
/// 0600 on Unix) before a single byte is written to it.
pub fn save(path: &Path, key: &Key, entries: &[Entry]) -> Result<()> {
    let data = seal(key, entries)?;
    let dir = path
        .parent()
        .ok_or_else(|| anyhow!("history path has no folder"))?;
    create_private_dir(dir)?;
    let tmp = path.with_extension("tmp");
    {
        let mut f = private_file(&tmp)?;
        f.write_all(&data)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path).with_context(|| format!("could not replace {}", path.display()))?;
    Ok(())
}

/// Remove the on-disk history, if any.
pub fn delete(path: &Path) -> Result<()> {
    for p in [path.to_path_buf(), path.with_extension("tmp")] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("could not remove {}", p.display())),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    // An older or hand-made folder may be more open; close it up.
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<()> {
    // Under the user's own profile folder, which Windows already limits to
    // that user.
    std::fs::create_dir_all(dir)?;
    Ok(())
}

#[cfg(unix)]
fn private_file(path: &Path) -> Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let _ = std::fs::remove_file(path);
    Ok(std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?)
}

#[cfg(not(unix))]
fn private_file(path: &Path) -> Result<std::fs::File> {
    Ok(std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries() -> Vec<Entry> {
        vec![Entry {
            id: 1,
            at: 1_700_000_000,
            target_id: "default".into(),
            target_label: "Focused Window".into(),
            text: "my bank PIN is not something to save in plain text".into(),
        }]
    }

    #[test]
    fn round_trips_with_the_right_key() {
        let key = new_key();
        let sealed = seal(&key, &entries()).unwrap();
        assert_eq!(open(&key, &sealed).unwrap(), entries());
    }

    #[test]
    fn the_text_is_not_in_the_file() {
        let sealed = seal(&new_key(), &entries()).unwrap();
        let haystack = String::from_utf8_lossy(&sealed);
        assert!(!haystack.contains("bank PIN"));
        assert!(!haystack.contains("Focused Window"));
    }

    #[test]
    fn a_wrong_key_or_tampering_is_refused() {
        let key = new_key();
        let mut sealed = seal(&key, &entries()).unwrap();
        assert!(open(&new_key(), &sealed).is_err());
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(open(&key, &sealed).is_err());
        assert!(open(&key, b"plain text").is_err());
    }

    #[test]
    fn save_load_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history").join("history.bin");
        let key = new_key();
        assert!(load(&path, &key).unwrap().is_empty());
        save(&path, &key, &entries()).unwrap();
        assert_eq!(load(&path, &key).unwrap(), entries());
        delete(&path).unwrap();
        assert!(!path.exists());
        delete(&path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn file_and_folder_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("history");
        let path = folder.join("history.bin");
        save(&path, &new_key(), &entries()).unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&folder), 0o700);
    }
}
