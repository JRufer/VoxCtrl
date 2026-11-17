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
use tracing::{info, warn};

use crate::piper::expand_tilde;

/// The audio.cpp release VoxCtrl downloads. Bump alongside a verified test of
/// the new release's server request fields for every family we drive.
///
/// v0.9.4 is the first release VoxCtrl uses that ships the `kitten_tts2`
/// family (Kitten TTS 2); the server config schema (`models[].path`,
/// `backend`, `lazy_load`) and the `/v1/audio/speech` fields VoxCtrl sends
/// (`voice`, `voice_ref`, `reference_text`, `instruct`, `options`) are the
/// same ones v0.8.0 accepted.
pub const AUDIOCPP_RELEASE_VERSION: &str = "v0.9.4";

/// Which of the release's Linux builds VoxCtrl installs.
///
/// Upstream publishes two Vulkan builds. The plain one is compiled for the
/// machine that built it and, as of v0.9.4, is full of AVX-512 code: on a CPU
/// without AVX-512 the server starts, then dies with SIGILL ("Illegal
/// instruction") on the first speech request, whichever backend was asked
/// for. The `-portable` build ships a ggml CPU backend per processor
/// generation as shared libraries and picks one at run time, so it runs on any
/// x86-64 CPU. It is also the smaller download.
const AUDIOCPP_LINUX_BUILD: &str = "ubuntu-x64-vulkan-portable";

/// What the version marker holds for the install VoxCtrl wants: the release
/// and the build. An install of the right release but the other build (the
/// non-portable one earlier VoxCtrl versions fetched) reads as out of date,
/// which is what gets it replaced.
fn wanted_install_id() -> String {
    format!("{AUDIOCPP_RELEASE_VERSION}-portable")
}

/// Written next to the managed binaries after a successful install, holding
/// the [`AUDIOCPP_RELEASE_VERSION`] that was installed. Installs from before
/// this marker existed have no file, which reads as out of date — so the next
/// audio.cpp model download also upgrades the runtime.
const AUDIOCPP_VERSION_MARKER: &str = "VOXCTRL_AUDIOCPP_VERSION";

/// GGUF model family names audio.cpp registers for the engines VoxCtrl offers.
pub const FAMILY_POCKET_TTS: &str = "pocket_tts";
pub const FAMILY_BREEZE_TTS: &str = "breeze_tts";
pub const FAMILY_VOXCPM2: &str = "voxcpm2";
pub const FAMILY_KITTEN_TTS2: &str = "kitten_tts2";

pub fn audiocpp_dir() -> PathBuf {
    dirs::data_local_dir().unwrap_or_else(|| PathBuf::from(".")).join("voxctrl").join("audiocpp")
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

/// Resolves the `audiocpp_server` binary: VoxCtrl's own managed install
/// first, then PATH — mirroring `piper_binary()`.
fn audiocpp_server_binary() -> Option<PathBuf> {
    let exe = if cfg!(target_os = "windows") { "audiocpp_server.exe" } else { "audiocpp_server" };
    let local = audiocpp_dir().join(exe);
    if local.exists() {
        return Some(local);
    }
    voxctrl_config::find_in_path("audiocpp_server")
}

/// What runtime the audio.cpp engines would run on right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCppRuntime {
    /// VoxCtrl's own install, at [`AUDIOCPP_RELEASE_VERSION`].
    Current,
    /// VoxCtrl's own install, from an older release (or from before the
    /// version marker existed). Works for the families that release had,
    /// but may lack newer ones such as `kitten_tts2`.
    Outdated,
    /// No managed install, but an `audiocpp_server` on PATH. The user manages
    /// that one; VoxCtrl never replaces it, and can't know what it supports.
    System,
    Missing,
}

fn installed_marker_version(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(AUDIOCPP_VERSION_MARKER))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn classify_runtime(managed_dir: &Path, on_path: bool) -> AudioCppRuntime {
    let exe = if cfg!(target_os = "windows") { "audiocpp_server.exe" } else { "audiocpp_server" };
    if managed_dir.join(exe).exists() {
        if installed_marker_version(managed_dir).as_deref() == Some(wanted_install_id().as_str()) {
            AudioCppRuntime::Current
        } else {
            AudioCppRuntime::Outdated
        }
    } else if on_path {
        AudioCppRuntime::System
    } else {
        AudioCppRuntime::Missing
    }
}

pub fn audiocpp_runtime() -> AudioCppRuntime {
    classify_runtime(&audiocpp_dir(), voxctrl_config::find_in_path("audiocpp_server").is_some())
}

/// Installs the audio.cpp runtime when it is missing, or upgrades VoxCtrl's
/// own install when it predates [`AUDIOCPP_RELEASE_VERSION`]. A system-wide
/// install on PATH is left alone. Every audio.cpp model download goes through
/// here, so asking for any of those models is what brings an older install
/// up to date.
pub async fn ensure_audiocpp_runtime() -> Result<()> {
    // One install at a time: startup, a model download and the first speech
    // request can all ask at once, and two would unpack over each other. The
    // state is read after the wait, so the second caller finds it done.
    static INSTALLING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _installing = INSTALLING.lock().await;
    match audiocpp_runtime() {
        AudioCppRuntime::Current | AudioCppRuntime::System => Ok(()),
        AudioCppRuntime::Outdated | AudioCppRuntime::Missing => {
            download_audiocpp_binary().await.context("download audio.cpp runtime")
        }
    }
}

/// [`ensure_audiocpp_runtime`] for the synchronous TTS worker thread, run
/// before a server is started. A model download brings the runtime up to date,
/// but someone whose models are already on disk never downloads one; without
/// this, an install that a VoxCtrl update has made out of date (and that may
/// not run at all on their CPU) would be started as it is, forever.
///
/// Best effort: offline, the old runtime is still better than none.
pub(crate) fn upgrade_outdated_runtime_blocking() {
    if audiocpp_runtime() != AudioCppRuntime::Outdated {
        return;
    }
    info!("The installed audio.cpp runtime is out of date; updating it before starting the speech engine");
    // On a thread of its own: a runtime cannot be started from a thread that
    // is already inside one, and this must not depend on who is calling.
    let result = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("start a runtime for the download")
                    .and_then(|rt| rt.block_on(ensure_audiocpp_runtime()))
            })
            .join()
    });
    match result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => warn!("Could not update the audio.cpp runtime ({e:#}); using the installed one"),
        Err(_) => warn!("Updating the audio.cpp runtime stopped unexpectedly; using the installed one"),
    }
}

// ── Binary download ───────────────────────────────────────────────────────────

/// Fetches and unpacks the prebuilt audio.cpp release archive for the current
/// OS into `~/.local/share/voxctrl/audiocpp/`, replacing any older install in
/// place. Only extracts the runtime (binary + shared libraries + model_specs +
/// license) — the archive also ships audio.cpp's own Python
/// model-manager/conversion tooling under `tools/`, which VoxCtrl never
/// invokes and does not install.
#[cfg(target_os = "linux")]
pub async fn download_audiocpp_binary() -> Result<()> {
    let dest_dir = audiocpp_dir();
    tokio::fs::create_dir_all(&dest_dir).await?;

    let asset = format!("audio-{AUDIOCPP_RELEASE_VERSION}-bin-{AUDIOCPP_LINUX_BUILD}.tar.gz");
    let url = format!("https://github.com/0xShug0/audio.cpp/releases/download/{AUDIOCPP_RELEASE_VERSION}/{asset}");
    info!("Downloading audio.cpp runtime: {url}");

    let response = reqwest::get(&url).await?.error_for_status()?;
    let bytes = response.bytes().await?;

    let extract_dir = dest_dir.clone();
    tokio::task::spawn_blocking(move || extract_audiocpp_archive(&bytes, &extract_dir))
        .await
        .context("extract_audiocpp_archive task join")??;
    // Last, so an interrupted install never claims to be current.
    tokio::fs::write(dest_dir.join(AUDIOCPP_VERSION_MARKER), wanted_install_id())
        .await
        .context("write audio.cpp version marker")?;

    info!("audio.cpp {AUDIOCPP_RELEASE_VERSION} runtime installed to {}", dest_dir.display());
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

        let rel: PathBuf = path.components().filter(|c| matches!(c, std::path::Component::Normal(_))).collect();
        if rel.as_os_str().is_empty() {
            continue;
        }

        let dest = dest_dir.join(&rel);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // Unlink before writing: on an upgrade, an older `audiocpp_server`
        // may still be running from this file (another engine's resident
        // session), and overwriting a running executable in place fails with
        // ETXTBSY. The running process keeps its unlinked copy until it exits.
        if entry.header().entry_type().is_file() {
            let _ = std::fs::remove_file(&dest);
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
    let rest = reference.strip_prefix("hf://").ok_or_else(|| anyhow::anyhow!("not an hf:// reference: {reference}"))?;
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
    let Ok((_repo, _url, _filename, dest)) = hf_reference_parts(reference) else {
        return Ok(PathBuf::from(reference));
    };
    if dest.exists() {
        return Ok(dest);
    }
    download_hf_file(reference, &dest, hf_token).await?;
    Ok(dest)
}

/// Downloads an `hf://owner/repo/path[@revision]` reference straight to
/// `dest` — for model GGUFs, which go into their engine's own model
/// directory rather than through the clip cache (a copy from the cache would
/// leave the same multi-gigabyte file on disk twice).
pub(crate) async fn download_hf_file(reference: &str, dest: &Path, hf_token: Option<&str>) -> Result<()> {
    let (repo, url, _filename, _cache_dest) = hf_reference_parts(reference)?;
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    // No overall timeout: these are multi-gigabyte GGUFs (Breeze-TTS-2's is
    // 5.1 GB, Kitten TTS 2's 3.3 GB), so any fixed total would fail on a slow
    // but healthy connection. A stalled connection is caught by the read
    // timeout instead.
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(120))
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
    stream_to_file(resp, dest).await.with_context(|| format!("download {url}"))
}

/// The in-progress name for a download headed to `dest`. It never ends in
/// `.gguf`, so audio.cpp (which loads every `*.gguf` in a model directory)
/// and VoxCtrl's ready checks both ignore a half-finished file.
fn partial_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

/// Writes a response body to `dest` chunk by chunk — never holding the whole
/// file in memory — via a `.part` file that is renamed into place only once
/// the body is complete.
async fn stream_to_file(mut resp: reqwest::Response, dest: &Path) -> Result<()> {
    use tokio::io::AsyncWriteExt;

    let part = partial_path(dest);
    let total = resp.content_length();
    let result: Result<()> = async {
        let mut file = tokio::fs::File::create(&part).await.with_context(|| format!("create {}", part.display()))?;
        let mut written: u64 = 0;
        let mut next_log: u64 = 0;
        while let Some(chunk) = resp.chunk().await.context("read response body")? {
            file.write_all(&chunk).await.with_context(|| format!("write {}", part.display()))?;
            written += chunk.len() as u64;
            if written >= next_log {
                match total {
                    Some(t) if t > 0 => info!("Downloaded {} / {} MB", written >> 20, t >> 20),
                    _ => info!("Downloaded {} MB", written >> 20),
                }
                next_log = written + (256 << 20);
            }
        }
        if let Some(t) = total {
            if written != t {
                anyhow::bail!("download ended early: got {written} of {t} bytes");
            }
        }
        file.flush().await?;
        file.sync_all().await?;
        Ok(())
    }
    .await;

    match result {
        Ok(()) => tokio::fs::rename(&part, dest).await.with_context(|| format!("move {} into place", part.display())),
        Err(e) => {
            let _ = tokio::fs::remove_file(&part).await;
            Err(e)
        }
    }
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
    /// Reference transcript for a `voice_ref` clip. `breeze_tts` and
    /// `kitten_tts2` require this whenever cloning; the other families treat
    /// it as an optional cloning-quality boost.
    pub reference_text: Option<&'a str>,
    /// Family-specific request options, sent as the body's `options` object
    /// (e.g. `kitten_tts2`'s `preset`). audio.cpp rejects option names a
    /// family's model spec doesn't declare, so only send ones it does.
    pub options: &'a [(&'a str, &'a str)],
}

/// A running `audiocpp_server` process bound to one model (family + model
/// directory + backend). Killed on drop, so dropping the `Option` that owns
/// one (e.g. on idle-unload, or when switching engine/GPU setting) frees
/// every resource the model held — not just its GPU/RAM weights.
pub(crate) struct AudioCppServer {
    // Behind a lock only so a failed request, which has `&self`, can ask how the
    // process ended.
    child: std::sync::Mutex<Child>,
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
        let child = self.child.get_mut().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn free_local_port() -> Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).context("bind ephemeral port")?;
    Ok(listener.local_addr()?.port())
}

/// Makes the spawned child ask the kernel to SIGTERM it the instant *this*
/// process dies, for any reason.
///
/// `AudioCppServer::drop` already kills its child on a graceful shutdown, but
/// that only runs if the TTS worker thread gets to process a
/// `TtsCommand::Shutdown` before the whole app exits — which several exit
/// paths (the tray's Quit item, the updater's relaunch, a crash, `kill -9`)
/// don't guarantee. Without this, a quit like that leaves `audiocpp_server`
/// (and, if the model had loaded, its GPU/RAM footprint) running forever.
/// `PR_SET_PDEATHSIG` is Linux-only, matching `download_audiocpp_binary`.
#[cfg(target_os = "linux")]
fn die_with_this_process(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;
    let niceness = HELPER_NICENESS.load(std::sync::atomic::Ordering::Relaxed);
    // SAFETY: `prctl` and `setpriority` are async-signal-safe and touch only
    // their own arguments — safe to call between fork and exec in the child.
    unsafe {
        cmd.pre_exec(move || {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            if niceness != 0 {
                // Best effort: a helper at normal priority is still a helper.
                libc::setpriority(libc::PRIO_PROCESS, 0, niceness);
            }
            Ok(())
        });
    }
}

/// The scheduling niceness audio.cpp helpers are started with. 0 = the same
/// priority as everything else.
static HELPER_NICENESS: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

/// Start audio.cpp helpers at a lower scheduling priority from now on
/// (`niceness` 1 to 19; 0 restores normal priority).
///
/// For a process that does other time-critical work beside speech: on a
/// server, neural synthesis on the CPU uses every core for seconds at a time,
/// and without this it competes on equal terms with transcription, which is
/// what the person at the other end is waiting on. Has no effect on helpers
/// already running, or outside Linux.
pub fn set_helper_niceness(niceness: i32) {
    HELPER_NICENESS.store(niceness.clamp(0, 19), std::sync::atomic::Ordering::Relaxed);
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

        let mut cmd = std::process::Command::new(&binary);
        cmd.arg("--config")
            .arg(config_file.path())
            .arg("--no-ui")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        #[cfg(target_os = "linux")]
        die_with_this_process(&mut cmd);

        let mut child = cmd.spawn().with_context(|| format!("spawn {}", binary.display()))?;

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
                anyhow::bail!("audiocpp_server exited during startup ({status:?}): {}", read_stderr(&mut child));
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                anyhow::bail!("audiocpp_server did not become ready in time: {}", read_stderr(&mut child));
            }
            std::thread::sleep(Duration::from_millis(150));
        }

        Ok(Self {
            child: std::sync::Mutex::new(child),
            base_url,
            http,
            family,
            model_dir: model_dir.to_path_buf(),
            gpu,
            _config_file: config_file,
        })
    }

    fn speak(&self, req: &SpeakRequest) -> Result<Vec<u8>> {
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
            reference_text: Option<&'a str>,
            #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
            options: std::collections::BTreeMap<&'a str, &'a str>,
        }

        let mut body = Body {
            model: self.family,
            input: req.text,
            voice: None,
            voice_ref: None,
            instruct: None,
            reference_text: req.reference_text,
            options: req.options.iter().copied().collect(),
        };
        match req.speaker {
            Some(SpeakerRef::Clone(clip)) => body.voice_ref = Some(clip),
            Some(SpeakerRef::VoiceId(id)) => body.voice = Some(id),
            // Every family that takes a Voice Design prompt (`voxcpm2`,
            // `breeze_tts`) reads it from the top-level `instruct` field on
            // audio.cpp v0.9.x. (Before, Breeze's prompt went in a
            // `request_options` object that the server never read.)
            Some(SpeakerRef::Design(prompt)) => body.instruct = Some(prompt),
            None => {}
        }

        let resp = self
            .http
            .post(format!("{}/v1/audio/speech", self.base_url))
            .json(&body)
            .send()
            .map_err(|e| self.explain_transport_failure(e))?;
        let status = resp.status();
        let bytes = resp.bytes().map_err(|e| self.explain_transport_failure(e))?;
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

/// If `child` has exited: how, and the end of what it printed, for the log.
/// `None` while it is still running.
fn exit_report(child: &mut Child) -> Option<String> {
    let status = match child.try_wait() {
        Ok(None) => return None,
        Ok(Some(status)) => status.to_string(),
        Err(e) => format!("its state could not be read: {e}"),
    };
    let stderr = read_stderr(child);
    // The reason is at the end; the start is startup chatter.
    let tail: String = {
        let chars: Vec<char> = stderr.chars().collect();
        chars[chars.len().saturating_sub(600)..].iter().collect()
    };
    Some(if tail.is_empty() { status } else { format!("{status}; last output: {tail}") })
}

impl AudioCppServer {
    /// A request that got no answer usually means the server process died
    /// under it. "Connection closed" says nothing a user can act on, so wait
    /// a moment for the process to finish exiting and say how it ended and
    /// what it last printed (out of memory, a missing library, a Vulkan
    /// failure, ...).
    fn explain_transport_failure(&self, error: reqwest::Error) -> anyhow::Error {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let deadline = Instant::now() + Duration::from_millis(1500);
        let report = loop {
            if let Some(report) = exit_report(&mut child) {
                break Some(report);
            }
            if Instant::now() > deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        match report {
            Some(report) => anyhow::anyhow!(
                "The audio.cpp speech engine ({}) stopped while working: {report}. It will be restarted on the next request.",
                self.family
            ),
            None => anyhow::Error::new(error).context(format!(
                "The audio.cpp speech engine ({}) did not answer, though it is still running",
                self.family
            )),
        }
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
        let matches = slot
            .as_ref()
            .is_some_and(|s| s.server.family == family && s.server.model_dir == model_dir && s.server.gpu == gpu);
        // A server that has exited (crashed, ran out of memory, was killed)
        // cannot be reused: without this check every later request would be
        // sent to a port nobody listens on, until the app restarted.
        let died = slot.as_mut().and_then(|s| exit_report(s.server.child.get_mut().unwrap_or_else(|e| e.into_inner())));
        if let Some(report) = &died {
            warn!("audio.cpp server (family={family}) is no longer running ({report}); starting a new one");
        }
        if matches && died.is_none() {
            return Ok(());
        }
        *slot = None; // drop first: frees the old port before the new server binds one
        upgrade_outdated_runtime_blocking();
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
#[cfg(feature = "playback")]
pub fn play_wav_bytes(sink: &rodio::Sink, bytes: Vec<u8>) -> Result<()> {
    let source = rodio::Decoder::new(std::io::Cursor::new(bytes)).context("decode audio.cpp response")?;
    sink.append(source);
    sink.sleep_until_end();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_running_helper_has_no_exit_report() {
        let mut child = std::process::Command::new("sleep").arg("30").stderr(Stdio::piped()).spawn().unwrap();
        assert_eq!(exit_report(&mut child), None);
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_helper_that_died_is_reported_with_how_and_its_last_words() {
        let mut child = std::process::Command::new("sh")
            .args(["-c", "echo 'out of memory loading model' >&2; exit 3"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // Poll rather than `wait()`: the report must come from `try_wait`.
        let report = loop {
            if let Some(report) = exit_report(&mut child) {
                break report;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(report.contains('3'), "{report}");
        assert!(report.contains("out of memory loading model"), "{report}");
    }

    #[cfg(unix)]
    #[test]
    fn a_killed_helper_is_reported_as_such() {
        let mut child = std::process::Command::new("sleep").arg("30").stderr(Stdio::piped()).spawn().unwrap();
        child.kill().unwrap();
        let report = loop {
            if let Some(report) = exit_report(&mut child) {
                break report;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        assert!(report.contains("signal"), "{report}");
    }
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
    fn test_runtime_missing_without_managed_install_or_path() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(classify_runtime(dir.path(), false), AudioCppRuntime::Missing);
        assert_eq!(classify_runtime(dir.path(), true), AudioCppRuntime::System);
    }

    #[test]
    fn test_runtime_without_marker_is_outdated() {
        // Every install made before the marker existed looks like this.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("audiocpp_server"), b"").unwrap();
        assert_eq!(classify_runtime(dir.path(), true), AudioCppRuntime::Outdated);
    }

    #[test]
    fn test_runtime_marker_must_match_release() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("audiocpp_server"), b"").unwrap();
        std::fs::write(dir.path().join(AUDIOCPP_VERSION_MARKER), "v0.8.0\n").unwrap();
        assert_eq!(classify_runtime(dir.path(), false), AudioCppRuntime::Outdated);
        // The right release, but the non-portable build earlier versions
        // installed: it must be replaced, so it is out of date too.
        std::fs::write(dir.path().join(AUDIOCPP_VERSION_MARKER), format!("{AUDIOCPP_RELEASE_VERSION}\n")).unwrap();
        assert_eq!(classify_runtime(dir.path(), false), AudioCppRuntime::Outdated);

        std::fs::write(dir.path().join(AUDIOCPP_VERSION_MARKER), format!("{}\n", wanted_install_id())).unwrap();
        assert_eq!(classify_runtime(dir.path(), false), AudioCppRuntime::Current);
    }

    #[test]
    fn test_partial_path_never_looks_like_a_gguf() {
        let p = partial_path(Path::new("/m/kitten-tts2-20261021-q8_0.gguf"));
        assert_eq!(p, Path::new("/m/kitten-tts2-20261021-q8_0.gguf.part"));
        assert_ne!(p.extension().and_then(|e| e.to_str()), Some("gguf"));
    }
}
