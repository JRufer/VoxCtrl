//! Shared runtime for every audio.cpp-backed engine (Pocket-TTS, Breeze-TTS-2,
//! VoxCPM2). All three are separate GGUF model "families" served by the same
//! `audiocpp_cli` binary from <https://github.com/0xShug0/audio.cpp> — a
//! ggml-based, Apache-2.0 C++ inference engine with a Vulkan backend.
//!
//! This replaces the previous in-process Candle (`pocket-tts` crate) runtime,
//! which pulled in `intel-mkl-src` — a proprietary-licensed redistribution of
//! Intel MKL incompatible with VoxCtrl's MIT license. Following the same
//! external-binary pattern already used for Piper (see `piper.rs`), VoxCtrl
//! spawns the prebuilt `audiocpp_cli` binary per utterance rather than linking
//! anything in-process, so there is no C++ build step and no Python: only the
//! prebuilt binary is invoked, and asset downloads go through VoxCtrl's own
//! `reqwest` code, never audio.cpp's Python model manager.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result};
use tracing::{info, warn};

use crate::piper::expand_tilde;

/// The audio.cpp release VoxCtrl downloads. Bump alongside a verified test of
/// the new release's CLI flags for the three families we drive.
pub const AUDIOCPP_RELEASE_VERSION: &str = "v0.8.0";

/// GGUF model family names audio.cpp registers for the engines VoxCtrl offers.
pub const FAMILY_POCKET_TTS: &str = "pocket_tts";
pub const FAMILY_BREEZE_TTS: &str = "breeze_tts";
pub const FAMILY_VOXCPM2: &str = "voxcpm2";

pub fn audiocpp_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("audiocpp")
}

/// Where VoxCtrl caches downloaded reference-voice clips (the `hf://` built-in
/// catalogue), keyed by their HuggingFace repo/path so a clip is fetched once.
fn voice_clip_cache_dir() -> PathBuf {
    audiocpp_dir().join("voice-clips")
}

pub(crate) fn resolve_model_dir(model_dir: &str, default: impl FnOnce() -> PathBuf) -> PathBuf {
    if model_dir.trim().is_empty() {
        default()
    } else {
        expand_tilde(model_dir.trim())
    }
}

/// Resolves the `audiocpp_cli` binary: VoxCtrl's own managed install first,
/// then PATH — mirroring `piper_binary()`.
pub fn audiocpp_binary() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "audiocpp_cli.exe" } else { "audiocpp_cli" };
    let local = audiocpp_dir().join(exe);
    if local.exists() {
        return Some(local);
    }
    voxctrl_config::find_in_path("audiocpp_cli")
}

// ── Binary download ───────────────────────────────────────────────────────────

/// Fetches and unpacks the prebuilt audio.cpp release archive for the current
/// OS into `~/.local/share/voxctrl/audiocpp/`. Only extracts the runtime
/// (binary + shared libraries + model_specs + license) — the archive also
/// ships audio.cpp's own Python model-manager/conversion tooling under
/// `tools/`, which VoxCtrl never invokes and does not install.
#[cfg(target_os = "linux")]
pub async fn download_audiocpp_binary() -> Result<()> {
    let dest_dir = audiocpp_dir();
    let dest_exe = dest_dir.join("audiocpp_cli");
    if dest_exe.exists() {
        return Ok(());
    }
    tokio::fs::create_dir_all(&dest_dir).await?;

    let asset = format!("audio-{AUDIOCPP_RELEASE_VERSION}-bin-ubuntu-x64-vulkan.tar.gz");
    let url = format!(
        "https://github.com/0xShug0/audio.cpp/releases/download/{AUDIOCPP_RELEASE_VERSION}/{asset}"
    );
    info!("Downloading audio.cpp runtime: {url}");

    let response = reqwest::get(&url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    tokio::task::spawn_blocking(move || extract_audiocpp_archive(&bytes, &dest_dir))
        .await
        .context("extract_audiocpp_archive task join")??;

    info!("audio.cpp runtime installed to {}", dest_exe.display());
    Ok(())
}

/// VoxCtrl has no verified Windows/macOS release asset name or install path
/// yet (see the audio.cpp Releases page), so — matching the precedent set by
/// `piper::download_piper_binary`'s Windows stub — this reports what it
/// cannot do instead of silently doing nothing.
#[cfg(not(target_os = "linux"))]
pub async fn download_audiocpp_binary() -> Result<()> {
    anyhow::bail!(
        "VoxCtrl cannot install audio.cpp automatically on this platform yet. Download a \
         release from https://github.com/0xShug0/audio.cpp/releases and place \
         audiocpp_cli{} in {}, or put it on PATH.",
        if cfg!(target_os = "windows") { ".exe" } else { "" },
        audiocpp_dir().display()
    );
}

#[cfg(target_os = "linux")]
fn extract_audiocpp_archive(bytes: &[u8], dest_dir: &Path) -> Result<()> {
    let cursor = std::io::Cursor::new(bytes);
    let tar = flate2::read::GzDecoder::new(cursor);
    let mut archive = tar::Archive::new(tar);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();

        // The archive ships everything at its own root (no wrapping
        // directory), alongside audio.cpp's Python model-manager/conversion
        // scripts under `tools/` — skip those, we only want the native runtime.
        if path.components().next().map(|c| c.as_os_str() == "tools").unwrap_or(false) {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) == Some("py") {
            continue;
        }

        let rel: PathBuf = path
            .components()
            .filter(|c| matches!(c, std::path::Component::Normal(_)))
            .collect();
        if rel.as_os_str().is_empty() {
            continue;
        }

        let dest = dest_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        entry.unpack(&dest)?;
    }

    use std::os::unix::fs::PermissionsExt;
    for exe in ["audiocpp_cli", "audiocpp_server"] {
        let path = dest_dir.join(exe);
        if let Ok(metadata) = std::fs::metadata(&path) {
            let mut perms = metadata.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&path, perms);
        }
    }

    Ok(())
}

// ── HuggingFace asset fetching (replaces `hf-hub`/`pocket_tts::weights`) ──────

/// Splits an `hf://owner/repo/path[@revision]` reference into its parts and
/// the local cache path it resolves to. Shared by the async, blocking, and
/// cache-lookup-only variants below.
fn hf_reference_parts(reference: &str) -> Result<(String, String, String, PathBuf)> {
    let rest = reference
        .strip_prefix("hf://")
        .ok_or_else(|| anyhow::anyhow!("not an hf:// reference: {reference}"))?;
    let mut parts = rest.splitn(3, '/');
    let (owner, repo, filename_rev) = match (parts.next(), parts.next(), parts.next()) {
        (Some(o), Some(r), Some(f)) => (o, r, f),
        _ => anyhow::bail!("malformed hf:// reference: {reference}"),
    };
    let (filename, revision) = match filename_rev.rfind('@') {
        Some(at) => (&filename_rev[..at], &filename_rev[at + 1..]),
        None => (filename_rev, "main"),
    };
    let dest = voice_clip_cache_dir().join(owner).join(repo).join(filename);
    let url = format!("https://huggingface.co/{owner}/{repo}/resolve/{revision}/{filename}");
    Ok((format!("{owner}/{repo}"), url, filename.to_string(), dest))
}

/// Downloads an `hf://owner/repo/path[@revision]` reference into VoxCtrl's own
/// cache, or passes a plain local path straight through. Returns the local
/// path either way. Async and `reqwest`-based, replacing the removed
/// `pocket_tts::weights::download_if_necessary` (which relied on `hf-hub`).
pub async fn resolve_hf_reference(reference: &str, hf_token: Option<&str>) -> Result<PathBuf> {
    let Ok((repo, url, _filename, dest)) = hf_reference_parts(reference) else {
        return Ok(PathBuf::from(reference));
    };
    if dest.exists() {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("build reqwest client")?;
    let mut request = client.get(&url);
    if let Some(token) = crate::hf::effective_hf_token(hf_token) {
        request = request.bearer_auth(token);
    }

    let resp = request.send().await.with_context(|| format!("fetch {url}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(crate::hf::token_rejected(&repo));
    }
    if !status.is_success() {
        anyhow::bail!("fetch {url}: HTTP {status}");
    }
    let bytes = resp.bytes().await.with_context(|| format!("read {url}"))?;
    tokio::fs::write(&dest, &bytes).await.with_context(|| format!("write {}", dest.display()))?;
    Ok(dest)
}

/// Blocking counterpart of [`resolve_hf_reference`], for the synchronous TTS
/// worker thread (`engine.rs` is not async) — mirrors the old `pocket-tts`
/// crate's own blocking download-on-first-use behavior for a reference voice
/// clip picked at speak time (e.g. from a Voice Design prompt).
pub(crate) fn resolve_hf_reference_blocking(reference: &str, hf_token: Option<&str>) -> Result<PathBuf> {
    let Ok((repo, url, _filename, dest)) = hf_reference_parts(reference) else {
        return Ok(PathBuf::from(reference));
    };
    if dest.exists() {
        return Ok(dest);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .context("build reqwest client")?;
    let mut request = client.get(&url);
    if let Some(token) = crate::hf::effective_hf_token(hf_token) {
        request = request.bearer_auth(token);
    }

    let resp = request.send().with_context(|| format!("fetch {url}"))?;
    let status = resp.status();
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return Err(crate::hf::token_rejected(&repo));
    }
    if !status.is_success() {
        anyhow::bail!("fetch {url}: HTTP {status}");
    }
    let bytes = resp.bytes().with_context(|| format!("read {url}"))?;
    std::fs::write(&dest, &bytes).with_context(|| format!("write {}", dest.display()))?;
    Ok(dest)
}

// ── Synthesis (spawns `audiocpp_cli` per utterance) ───────────────────────────

/// Either a reference clip to clone (`--voice-ref`) or a natural-language
/// Voice Design instruction (`--instruct`) — audio.cpp's two ways to pick a
/// speaker identity for a cloning-capable family.
pub enum SpeakerRef<'a> {
    Clone(&'a Path),
    Design(&'a str),
}

pub struct SynthesizeRequest<'a> {
    pub family: &'static str,
    pub model_dir: &'a Path,
    pub gpu: bool,
    pub text: &'a str,
    pub speaker: Option<SpeakerRef<'a>>,
    /// Reference transcript for the `--voice-ref` clip (`--reference-text`).
    /// `breeze_tts` requires this whenever cloning; the other families treat
    /// it as an optional cloning-quality boost.
    pub reference_text: Option<&'a str>,
    pub load_options: &'a [(&'a str, &'a str)],
}

/// Synthesizes `req.text` into a WAV file at `out_wav` by spawning
/// `audiocpp_cli`, mirroring `TtsEngineWorker::speak_piper`'s external-process
/// pattern. `--backend vulkan` is used when `gpu` is set, falling back to
/// `--backend cpu` — audio.cpp reports a missing/unsupported Vulkan device as
/// a normal command failure, so a caller that wants a soft fallback should
/// retry once with `gpu: false`.
pub fn synthesize(req: &SynthesizeRequest, out_wav: &Path) -> Result<()> {
    let binary = audiocpp_binary().ok_or_else(|| {
        anyhow::anyhow!(
            "audio.cpp binary not found. Download it from TTS settings, or install \
             audiocpp_cli system-wide."
        )
    })?;

    let mut cmd = std::process::Command::new(&binary);
    cmd.arg("--task")
        .arg("tts")
        .arg("--family")
        .arg(req.family)
        .arg("--model")
        .arg(req.model_dir)
        .arg("--backend")
        .arg(if req.gpu { "vulkan" } else { "cpu" })
        .arg("--text")
        .arg(req.text)
        .arg("--out")
        .arg(out_wav);

    match req.speaker {
        Some(SpeakerRef::Clone(clip)) => {
            cmd.arg("--voice-ref").arg(clip);
        }
        Some(SpeakerRef::Design(prompt)) => {
            // `--instruct` works as a direct CLI flag for most families
            // (verified against voxcpm2), but audio.cpp v0.8.0's breeze_tts
            // wiring only accepts the same instruction through its own
            // `instruction` request option — passing `--instruct` there
            // fails with "unknown BreezeTTS request option: instruct".
            if req.family == FAMILY_BREEZE_TTS {
                cmd.arg("--request-option").arg(format!("instruction={prompt}"));
            } else {
                cmd.arg("--instruct").arg(prompt);
            }
        }
        None => {}
    }
    if let Some(text) = req.reference_text {
        cmd.arg("--reference-text").arg(text);
    }
    for (key, value) in req.load_options {
        cmd.arg("--load-option").arg(format!("{key}={value}"));
    }

    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .context("spawn audiocpp_cli")?;

    if !output.status.success() {
        let err_msg = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "audiocpp_cli (family={}) failed with exit code {:?}: {}",
            req.family,
            output.status.code(),
            err_msg.trim()
        );
    }
    if !out_wav.exists() {
        warn!("audiocpp_cli reported success but wrote no output file at {}", out_wav.display());
        anyhow::bail!("audiocpp_cli produced no output audio");
    }

    Ok(())
}

/// Decodes a WAV file audio.cpp produced and queues it onto `sink`, blocking
/// until playback ends — the counterpart to `engine::play_raw_audio` for the
/// file-based (rather than raw-PCM-stdout) audio.cpp path.
pub fn play_wav_file(sink: &rodio::Sink, path: &Path) -> Result<()> {
    let file = std::fs::File::open(path).with_context(|| format!("open {}", path.display()))?;
    let source = rodio::Decoder::new(std::io::BufReader::new(file))
        .with_context(|| format!("decode {}", path.display()))?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_hf_reference_parts_parses_owner_repo_file() {
        let (repo, url, filename, _dest) =
            hf_reference_parts("hf://kyutai/tts-voices/alba-mackenna/casual.wav").unwrap();
        assert_eq!(repo, "kyutai/tts-voices");
        assert_eq!(filename, "alba-mackenna/casual.wav");
        assert!(url.starts_with("https://huggingface.co/kyutai/tts-voices/resolve/main/"));
    }

    #[test]
    fn test_hf_reference_parts_rejects_non_hf_uri() {
        assert!(hf_reference_parts("/plain/local/path.wav").is_err());
    }

    #[test]
    fn test_resolve_model_dir_empty_uses_default() {
        let dir = tempdir().unwrap();
        let default_dir = dir.path().to_path_buf();
        let result = resolve_model_dir("", || default_dir.clone());
        assert_eq!(result, default_dir);
    }

    #[test]
    fn test_resolve_model_dir_explicit_expands_tilde() {
        let home = dirs::home_dir().unwrap();
        let result = resolve_model_dir("~/my-model", || PathBuf::from("unused"));
        assert_eq!(result, home.join("my-model"));
    }

    #[test]
    fn test_audiocpp_binary_returns_option_without_panicking() {
        let _ = audiocpp_binary();
    }
}
