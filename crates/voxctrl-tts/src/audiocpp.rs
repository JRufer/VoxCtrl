//! Shared runtime for every audio.cpp-backed engine (Pocket-TTS, Breeze-TTS-2,
//! VoxCPM2). All three are separate GGUF model "families" served by the same
//! audio.cpp binaries from <https://github.com/0xShug0/audio.cpp> — a
//! ggml-based, Apache-2.0 C++ inference engine with a Vulkan backend.
//!
//! This replaces the previous in-process Candle (`pocket-tts` crate) runtime,
//! which pulled in `intel-mkl-src` — a proprietary-licensed redistribution of
//! Intel MKL incompatible with VoxCtrl's MIT license. VoxCtrl downloads the
//! prebuilt binaries rather than linking anything in-process, so there is no
//! C++ build step and no Python: only prebuilt binaries are invoked, and
//! asset downloads go through VoxCtrl's own `reqwest` code, never audio.cpp's
//! Python model manager.
//!
//! Synthesis itself runs through a long-lived `audiocpp_server` process
//! ([`AudioCppServer`]/[`AudioCppSession`]) rather than a fresh `audiocpp_cli`
//! spawn per utterance: loading a multi-gigabyte GGUF model dominates a
//! one-shot CLI call's wall time (measured: ~4s of a ~5s Breeze-TTS-2 request
//! was model load, ~1s was actual generation), so keeping the model resident
//! across utterances is the difference between a 5s and a 1s reply. The
//! server's own `lazy_load` defers the load to the first request, so a
//! `TtsMemoryMode::OnDemand`-driven idle-unload (dropping the `AudioCppSession`,
//! which kills the child process) still gives the memory back, exactly like
//! Inflect-Micro-v2 today — it just pays the load cost again on next use.

use std::io::Read as _;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;
use tracing::info;

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

/// Resolves the `audiocpp_server` binary the same way as [`audiocpp_binary`].
fn audiocpp_server_binary() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "audiocpp_server.exe" } else { "audiocpp_server" };
    let local = audiocpp_dir().join(exe);
    if local.exists() {
        return Some(local);
    }
    voxctrl_config::find_in_path("audiocpp_server")
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

// ── Synthesis (persistent `audiocpp_server` session) ──────────────────────────

/// Either a reference clip to clone (`voice_ref`), a built-in voice id
/// (`voice`), or a natural-language Voice Design instruction — audio.cpp's
/// ways to pick a speaker identity for a cloning-capable family.
pub enum SpeakerRef<'a> {
    Clone(&'a Path),
    VoiceId(&'a str),
    Design(&'a str),
}

pub struct SpeakRequest<'a> {
    pub text: &'a str,
    pub speaker: Option<SpeakerRef<'a>>,
    /// Reference transcript for a `voice_ref` clip. `breeze_tts` requires
    /// this whenever cloning; the other families treat it as an optional
    /// cloning-quality boost.
    pub reference_text: Option<&'a str>,
}

/// A running `audiocpp_server` process bound to one model (family + model
/// directory + backend). Killed on drop, so dropping the `Option` that owns
/// one (e.g. on idle-unload, or when switching engine/GPU setting) frees
/// every resource the model held — not just its GPU/RAM weights.
pub(crate) struct AudioCppServer {
    child: Child,
    base_url: String,
    http: reqwest::blocking::Client,
    family: &'static str,
    model_dir: PathBuf,
    gpu: bool,
    // Kept alive for the server's lifetime — it reads this file once at
    // startup, but only after `spawn()` returns; dropping it any earlier
    // would race the child's own read of the file.
    _config_file: tempfile::NamedTempFile,
}

impl Drop for AudioCppServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_local_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind ephemeral port")?;
    Ok(listener.local_addr()?.port())
}

impl AudioCppServer {
    /// Spawns `audiocpp_server` configured with exactly one model — the one
    /// this session exists for — and waits for it to answer `/health`.
    /// Model weights are *not* loaded yet at this point (`lazy_load: true`):
    /// that happens lazily on the first `speak()` call.
    fn spawn(family: &'static str, model_dir: &Path, gpu: bool) -> Result<Self> {
        let binary = audiocpp_server_binary().ok_or_else(|| {
            anyhow::anyhow!(
                "audio.cpp server binary not found. Download it from TTS settings, or install \
                 audiocpp_server system-wide."
            )
        })?;

        let port = free_local_port().context("find a free port for audiocpp_server")?;
        let config = serde_json::json!({
            "host": "127.0.0.1",
            "port": port,
            "backend": if gpu { "vulkan" } else { "cpu" },
            "lazy_load": true,
            "models": [{
                "id": family,
                "family": family,
                "path": model_dir,
                "task": "tts",
                "mode": "offline",
            }],
        });
        let mut config_file = tempfile::Builder::new()
            .prefix("voxctrl-audiocpp-server-")
            .suffix(".json")
            .tempfile()
            .context("create audiocpp_server config file")?;
        {
            use std::io::Write;
            config_file
                .write_all(serde_json::to_string_pretty(&config)?.as_bytes())
                .context("write audiocpp_server config file")?;
            config_file.flush()?;
        }

        let mut child = std::process::Command::new(&binary)
            .arg("--config")
            .arg(config_file.path())
            .arg("--no-ui")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawn {}", binary.display()))?;

        let base_url = format!("http://127.0.0.1:{port}");
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .context("build reqwest client")?;

        // Poll for readiness rather than sleeping a fixed amount: startup
        // time depends on the host, and this only waits for the HTTP
        // listener, not the (lazy-loaded) model itself.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(resp) = http.get(format!("{base_url}/health")).send() {
                if resp.status().is_success() {
                    break;
                }
            }
            if let Ok(Some(status)) = child.try_wait() {
                anyhow::bail!(
                    "audiocpp_server exited during startup ({status:?}): {}",
                    read_stderr(&mut child)
                );
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                anyhow::bail!("audiocpp_server did not become ready in time: {}", read_stderr(&mut child));
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        Ok(Self { child, base_url, http, family, model_dir: model_dir.to_path_buf(), gpu, _config_file: config_file })
    }

    fn speak(&self, req: &SpeakRequest) -> Result<Vec<u8>> {
        #[derive(Serialize)]
        struct RequestOptions<'a> {
            instruction: &'a str,
        }
        #[derive(Serialize)]
        struct Body<'a> {
            model: &'static str,
            input: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            voice: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            voice_ref: Option<&'a Path>,
            #[serde(skip_serializing_if = "Option::is_none")]
            instruct: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            request_options: Option<RequestOptions<'a>>,
            #[serde(skip_serializing_if = "Option::is_none")]
            reference_text: Option<&'a str>,
        }

        let mut body = Body {
            model: self.family,
            input: req.text,
            voice: None,
            voice_ref: None,
            instruct: None,
            request_options: None,
            reference_text: req.reference_text,
        };
        match req.speaker {
            Some(SpeakerRef::Clone(clip)) => body.voice_ref = Some(clip),
            Some(SpeakerRef::VoiceId(id)) => body.voice = Some(id),
            Some(SpeakerRef::Design(prompt)) => {
                // Confirmed against a running server: `voxcpm2` honors the
                // top-level `instruct` field structurally (though — separate,
                // upstream issue — it doesn't yet audibly act on it), while
                // `breeze_tts` only reads its own `instruction` request
                // option (mirrors the equivalent audiocpp_cli quirk: passing
                // `--instruct` to breeze_tts fails outright there).
                if self.family == FAMILY_BREEZE_TTS {
                    body.request_options = Some(RequestOptions { instruction: prompt });
                } else {
                    body.instruct = Some(prompt);
                }
            }
            None => {}
        }

        let resp = self
            .http
            .post(format!("{}/v1/audio/speech", self.base_url))
            .json(&body)
            .send()
            .context("POST /v1/audio/speech")?;
        let status = resp.status();
        let bytes = resp.bytes().context("read /v1/audio/speech response")?;
        if !status.is_success() {
            let message = serde_json::from_slice::<serde_json::Value>(&bytes)
                .ok()
                .and_then(|v| v.get("error")?.get("message")?.as_str().map(str::to_string))
                .unwrap_or_else(|| String::from_utf8_lossy(&bytes).trim().to_string());
            anyhow::bail!("audiocpp_server (family={}) request failed: {message}", self.family);
        }
        Ok(bytes.to_vec())
    }
}

fn read_stderr(child: &mut Child) -> String {
    let mut stderr = String::new();
    if let Some(mut s) = child.stderr.take() {
        let _ = s.read_to_string(&mut stderr);
    }
    stderr.trim().to_string()
}

/// A resident audio.cpp model session, reused across utterances for as long
/// as the family/model directory/backend it was spawned for stays current.
pub(crate) struct AudioCppSession {
    server: AudioCppServer,
}

impl AudioCppSession {
    /// Ensures `slot` holds a session for `family`/`model_dir`/`gpu`,
    /// spawning one if there is none, or replacing it (killing the old
    /// server) if any of those changed under it — mirroring how a GPU
    /// setting change already forced a model reload before this session
    /// concept existed.
    pub(crate) fn ensure(
        slot: &mut Option<AudioCppSession>,
        family: &'static str,
        model_dir: &Path,
        gpu: bool,
    ) -> Result<()> {
        let matches = slot.as_ref().is_some_and(|s| {
            s.server.family == family && s.server.model_dir == model_dir && s.server.gpu == gpu
        });
        if matches {
            return Ok(());
        }
        *slot = None; // drop first: frees the old port before the new server binds one
        info!("Starting audio.cpp server session (family={family}, gpu={gpu})");
        *slot = Some(AudioCppSession { server: AudioCppServer::spawn(family, model_dir, gpu)? });
        Ok(())
    }

    pub(crate) fn speak(&self, req: &SpeakRequest) -> Result<Vec<u8>> {
        self.server.speak(req)
    }
}

/// Decodes WAV bytes audio.cpp's server returned and queues them onto
/// `sink`, blocking until playback ends.
pub fn play_wav_bytes(sink: &rodio::Sink, bytes: Vec<u8>) -> Result<()> {
    let source = rodio::Decoder::new(std::io::Cursor::new(bytes)).context("decode audio.cpp response")?;
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
