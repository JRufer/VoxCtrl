//! Small helpers shared by the transcription backends.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

/// How many threads a transcription backend should run on.
///
/// Physical cores, not logical ones: whisper.cpp and ONNX Runtime are both
/// memory-bandwidth bound here, and two hyperthreads sharing one core's load
/// units finish no faster than one while costing the scheduler a context to
/// juggle. Counting cores directly also stops a machine without SMT — most
/// ARM laptops, and any desktop with it switched off — from being told to use
/// half its CPU, which is what dividing the logical count by two did.
///
/// Computed once: `get_physical` reads sysfs (or the platform equivalent) and
/// the answer does not change while the app runs.
pub(crate) fn inference_threads() -> usize {
    static THREADS: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *THREADS.get_or_init(|| {
        let physical = num_cpus::get_physical();
        if physical > 0 {
            physical
        } else {
            // Detection failed; fall back to the old logical-core heuristic.
            std::thread::available_parallelism().map(|n| (n.get() / 2).max(1)).unwrap_or(2)
        }
    })
}

/// Base directory for on-device models: `<data-local>/voxctrl/models`.
/// Each backend places its files in a subfolder of this (whisper-cpp directly,
/// Moonshine under `moonshine/`).
pub(crate) fn models_base_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("voxctrl").join("models")
}

/// Expand a leading `~` / `~/` to the user's home directory. Any other path is
/// returned unchanged.
pub(crate) fn expand_tilde(path: &str) -> PathBuf {
    let home = std::env::var("HOME").map(PathBuf::from).ok().or_else(dirs::home_dir);
    if path == "~" {
        return home.unwrap_or_else(|| PathBuf::from("~"));
    }
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(h) = home {
            return h.join(rest);
        }
    }
    PathBuf::from(path)
}

// ── Model downloads ───────────────────────────────────────────────────────────

/// Stream `url` to `path` through a `.part` file, renamed into place only once
/// the whole body has arrived.
///
/// Every model download goes through here. Streaming keeps memory flat: the
/// files run from 75 MB to 3.1 GB (Whisper `large-v3`), and buffering one with
/// `Response::bytes()` holds all of it in RAM before a byte reaches disk, which
/// a small server does not have.
/// When the server sends a length, a body that ends short is rejected rather
/// than renamed into place, where the backend's `is_model_downloaded` would later take it
/// for a finished file. A failed attempt removes its `.part` file.
pub(crate) async fn download_file(url: &str, path: &Path) -> Result<()> {
    let tmp = path.with_extension("part");
    let result = stream_to(url, &tmp).await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&tmp).await;
    }
    result?;
    tokio::fs::rename(&tmp, path).await.with_context(|| format!("finalize {}", path.display()))
}

async fn stream_to(url: &str, tmp: &Path) -> Result<()> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let response = reqwest::get(url)
        .await
        .with_context(|| format!("request {url}"))?
        .error_for_status()
        .with_context(|| format!("fetch {url}"))?;
    let expected = response.content_length();

    let mut file = tokio::fs::File::create(tmp).await.with_context(|| format!("create {}", tmp.display()))?;
    let mut written: u64 = 0;
    let mut body = response.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk.with_context(|| format!("read {url}"))?;
        file.write_all(&chunk).await.with_context(|| format!("write {}", tmp.display()))?;
        written += chunk.len() as u64;
    }
    file.flush().await.with_context(|| format!("flush {}", tmp.display()))?;
    file.sync_all().await.with_context(|| format!("sync {}", tmp.display()))?;

    if let Some(expected) = expected {
        if written != expected {
            bail!("download of {url} ended early: got {written} of {expected} bytes");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde_home() {
        let home = dirs::home_dir().expect("home dir must be available");
        assert_eq!(expand_tilde("~"), home);
    }

    #[test]
    fn test_expand_tilde_subdir() {
        let home = dirs::home_dir().expect("home dir must be available");
        assert_eq!(expand_tilde("~/.models"), home.join(".models"));
    }

    #[test]
    fn test_expand_tilde_absolute_unchanged() {
        assert_eq!(expand_tilde("/tmp/models"), PathBuf::from("/tmp/models"));
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("relative/path"), PathBuf::from("relative/path"));
    }

    #[test]
    fn test_models_base_dir_ends_with_models() {
        assert!(models_base_dir().ends_with("models"));
    }
}
