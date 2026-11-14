pub mod backend;
pub mod bench;
pub mod finalize;
#[cfg(feature = "moonshine")]
pub mod moonshine;
#[cfg(feature = "parakeet")]
pub mod parakeet;
pub mod postprocess;
pub mod remote_openai;
pub mod remote_status;
pub mod s1_mini;
mod util;
pub mod whisper_cpp;

pub use remote_openai::{
    probe_remote_speech_engine, test_remote_speech_engine, RemoteOpenAiBackend, RemoteSttTestResult,
    RemoteStreamingSession,
};

/// Whether the Moonshine ONNX backend was compiled into this build. When false,
/// selecting Moonshine transparently falls back to whisper-cpp, and callers
/// (e.g. the "model not downloaded" UI checks) must treat a Moonshine selection
/// as effectively whisper-cpp.
pub const MOONSHINE_COMPILED: bool = cfg!(feature = "moonshine");

/// Whether the Parakeet ONNX backend was compiled into this build. When false,
/// selecting Parakeet transparently falls back to whisper-cpp.
pub const PARAKEET_COMPILED: bool = cfg!(feature = "parakeet");

/// Which GPU backend whisper.cpp can offload to in this build, or `None` for a
/// CPU-only build.
///
/// This is fixed when the binary is compiled — ggml links exactly one compute
/// backend — so it is the honest answer to "can this app use my GPU", and the
/// only correct source for the Device dropdown. `whisper_cpp.device` selects
/// *whether* to offload, never *to what*: a config asking for `cuda` on a
/// Vulkan build gets Vulkan, and on a CPU build gets nothing at all.
pub fn whisper_gpu_backend() -> Option<&'static str> {
    // Both features can be enabled at once; ggml picks CUDA in that case.
    if cfg!(feature = "cuda") {
        Some("cuda")
    } else if cfg!(feature = "vulkan") {
        Some("vulkan")
    } else {
        None
    }
}

/// Which GPU backend the Moonshine ONNX backend can offload to in this build,
/// or `None` when it runs on the CPU.
///
/// ONNX Runtime has no Vulkan execution provider, so a Vulkan build — the one
/// that gives whisper.cpp GPU offload without CUDA's runtime — has no GPU path
/// for Moonshine at all. That asymmetry is why this is reported separately from
/// [`whisper_gpu_backend`] rather than inferred from it: they are genuinely
/// different answers in the build most people run.
pub fn moonshine_gpu_backend() -> Option<&'static str> {
    // Same order as `MoonshineBackend::with_gpu` registers them in. The two
    // being one edit apart is the whole risk here, so a test holds them
    // together rather than a comment alone.
    if cfg!(feature = "moonshine-cuda") {
        Some("cuda")
    } else if cfg!(feature = "moonshine-coreml") {
        Some("coreml")
    } else if cfg!(feature = "moonshine-webgpu") {
        Some("webgpu")
    } else {
        None
    }
}

/// The same provider, spelled the way ONNX Runtime spells it, for registration
/// and logging.
///
/// A pure mapping over [`moonshine_gpu_backend`] rather than a second `cfg`
/// cascade: the Engine tab and the session builder then cannot name different
/// providers, which is the failure mode worth designing out. `with_gpu` still
/// selects the provider *type* by `cfg`, but on exactly the conditions above.
// Its only non-test caller is the branch of `with_gpu` that registers a
// provider, which is compiled out when no GPU feature is on — the `moonshine`
// feature alone is not enough to reach it, which is what the first version of
// this attribute got wrong. The tests below still exercise it either way.
#[cfg_attr(
    not(any(
        feature = "moonshine-cuda",
        feature = "moonshine-coreml",
        feature = "moonshine-webgpu"
    )),
    allow(dead_code)
)]
pub(crate) fn moonshine_gpu_provider() -> Option<&'static str> {
    match moonshine_gpu_backend() {
        Some("cuda") => Some("CUDA"),
        Some("coreml") => Some("CoreML"),
        Some("webgpu") => Some("WebGPU"),
        _ => None,
    }
}

/// Which GPU backend the Parakeet ONNX backend can offload to in this build,
/// or `None` when it runs on the CPU.
pub fn parakeet_gpu_backend() -> Option<&'static str> {
    if cfg!(feature = "parakeet-cuda") {
        Some("cuda")
    } else if cfg!(feature = "parakeet-coreml") {
        Some("coreml")
    } else if cfg!(feature = "parakeet-webgpu") {
        Some("webgpu")
    } else {
        None
    }
}

/// Which GPU backend S1-mini can offload to in this build, or `None` when running on the CPU.
pub fn s1_mini_gpu_backend() -> Option<&'static str> {
    crate::s1_mini::sidecar_gpu_backend()
}

#[cfg_attr(
    not(any(
        feature = "parakeet-cuda",
        feature = "parakeet-coreml",
        feature = "parakeet-webgpu"
    )),
    allow(dead_code)
)]
pub(crate) fn parakeet_gpu_provider() -> Option<&'static str> {
    match parakeet_gpu_backend() {
        Some("cuda") => Some("CUDA"),
        Some("coreml") => Some("CoreML"),
        Some("webgpu") => Some("WebGPU"),
        _ => None,
    }
}

#[cfg(test)]
mod gpu_backend_tests {
    use super::*;

    #[test]
    fn every_reported_backend_has_a_provider_spelling() {
        // A backend the mapping does not know would make `with_gpu` log a bare
        // "GPU" while registering something specific — the Engine tab and the
        // log disagreeing about what this build does. Runs in every feature
        // configuration CI builds, including the default CPU one.
        match moonshine_gpu_backend() {
            Some(backend) => assert!(
                moonshine_gpu_provider().is_some(),
                "{backend} is reported to the UI but has no ONNX Runtime spelling"
            ),
            None => assert_eq!(
                moonshine_gpu_provider(),
                None,
                "a CPU-only build named a GPU provider"
            ),
        }
        match parakeet_gpu_backend() {
            Some(backend) => assert!(
                parakeet_gpu_provider().is_some(),
                "{backend} is reported to the UI but has no ONNX Runtime spelling"
            ),
            None => assert_eq!(
                parakeet_gpu_provider(),
                None,
                "a CPU-only build named a GPU provider"
            ),
        }
    }

    #[test]
    fn a_build_with_no_gpu_feature_reports_none() {
        if cfg!(not(any(
            feature = "moonshine-cuda",
            feature = "moonshine-coreml",
            feature = "moonshine-webgpu"
        ))) {
            assert_eq!(moonshine_gpu_backend(), None);
        }
        if cfg!(not(any(
            feature = "parakeet-cuda",
            feature = "parakeet-coreml",
            feature = "parakeet-webgpu"
        ))) {
            assert_eq!(parakeet_gpu_backend(), None);
        }
    }

    #[test]
    fn the_webgpu_build_reports_webgpu() {
        // Guards the release artifact: a `moonshine-webgpu` build that reported
        // None would show "CPU only" in the Engine tab on the very build that
        // exists to use the GPU.
        if cfg!(all(
            feature = "moonshine-webgpu",
            not(any(feature = "moonshine-cuda", feature = "moonshine-coreml"))
        )) {
            assert_eq!(moonshine_gpu_backend(), Some("webgpu"));
            assert_eq!(moonshine_gpu_provider(), Some("WebGPU"));
        }
    }
}

use std::sync::Arc;

use anyhow::Result;
use crossbeam_channel::{Receiver, Sender};
use tracing::{error, info};
use voxctrl_config::{AppConfig, BackendChoice};

use backend::{TranscribeRequest, TranscriptionBackend};
use whisper_cpp::WhisperCppBackend;

// ── Audio chunk type (must match voxctrl-audio) ────────────────────────────────

pub type AudioChunk = Vec<f32>;

// ── Cancelling live passes ────────────────────────────────────────────────────

/// The newest recording session whose live passes are no longer wanted.
/// Sessions count up from 1, so 0 cancels nothing.
static INTERIM_CANCELLED_UP_TO: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Stop every live pass (live text, early command detection) for `session`
/// and the sessions before it.
///
/// Called the moment a recording ends. Those passes only serve the overlay
/// and the command head start while the user is still talking; afterwards
/// the final pass over the whole recording replaces them. One already running
/// would otherwise hold the single inference thread, and the final pass would
/// wait for it before it could even start. Queued passes are skipped; a
/// running whisper.cpp pass stops part-way. Never affects the final pass.
pub fn cancel_interim_passes(session: u64) {
    INTERIM_CANCELLED_UP_TO.fetch_max(session, std::sync::atomic::Ordering::SeqCst);
}

/// Whether live passes for `session` have been cancelled.
pub fn interim_cancelled(session: u64) -> bool {
    session <= INTERIM_CANCELLED_UP_TO.load(std::sync::atomic::Ordering::SeqCst)
}

/// The error a cancelled live pass reports. The caller treats an interim
/// error as "this pass produced nothing", which is exactly right here.
const CANCELLED: &str = "live pass cancelled: the recording ended";

/// The error a live pass reports when there is nothing to run it on.
const NO_LIVE_ENGINE: &str = "live pass skipped: the server is unreachable and no fallback engine is set";

// ── Inference request ─────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// Accumulated audio samples (16 kHz, mono, f32)
    pub audio: Vec<f32>,
    /// Target id (used to look up per-target processing overrides)
    pub target_id: String,
    /// Hotkey binding ID (if triggered by a hotkey)
    pub binding_id: Option<String>,
    /// Whether this is an intermediate/live transcription during active speech
    pub is_interim: bool,
    /// Unique session identifier for the recording turn
    pub session_id: u64,
    /// Post-processing overrides of the utterance's primary target (see
    /// [`finalize::target_processing`]). Resolved by the caller, so this crate
    /// never reads routing's config files itself.
    pub processing: voxctrl_routing::TargetProcessingConfig,
    /// The hotkey binding that started the recording, for its per-hotkey
    /// OpenAI rewrite settings. `None` when no hotkey did.
    pub binding: Option<voxctrl_routing::HotkeyBinding>,
}

/// Final output after transcription + post-processing.
#[derive(Debug, Clone)]
pub struct InferenceOutput {
    pub text: String,
    pub target_id: String,
    pub binding_id: Option<String>,
    pub raw_text: String,
    pub inference_ms: u32,
    pub language: String,
    /// Set when transcription failed (model missing, backend error, ...). The
    /// UI layer surfaces this to the user; `text` is empty in that case.
    pub error: Option<String>,
    pub is_interim: bool,
    pub session_id: u64,
}

impl InferenceOutput {
    /// An utterance that produced no text (silence, or nothing to transcribe).
    pub fn empty(
        target_id: String,
        binding_id: Option<String>,
        is_interim: bool,
        session_id: u64,
    ) -> Self {
        Self {
            text: String::new(),
            target_id,
            binding_id,
            raw_text: String::new(),
            inference_ms: 0,
            language: String::new(),
            error: None,
            is_interim,
            session_id,
        }
    }

    /// An utterance whose transcription failed with `error`.
    pub fn failed(
        target_id: String,
        binding_id: Option<String>,
        error: &anyhow::Error,
        is_interim: bool,
        session_id: u64,
    ) -> Self {
        Self {
            error: Some(format!("{error:#}")),
            ..Self::empty(target_id, binding_id, is_interim, session_id)
        }
    }
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct InferenceEngine {
    config: Arc<AppConfig>,
    backend: Box<dyn TranscriptionBackend>,
    /// The remote server, for live passes only (live text, early command
    /// detection). `Some` exactly when a remote backend is selected. The final
    /// pass of a remote dictation does not come through this engine at all —
    /// the recording pipeline streams it to the server itself — so without
    /// this a remote backend had no live text.
    live_remote: Option<Box<dyn TranscriptionBackend>>,
}

/// Which engine transcribes one pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassRoute {
    /// The on-device backend this engine holds.
    Local,
    /// The remote server.
    Remote,
    /// Nothing: a live pass with the server unreachable and no fallback.
    Skip,
}

/// Decide where a pass goes.
///
/// A local backend transcribes everything itself. With a remote backend, a
/// final pass that reaches this engine is one the server could not take, so it
/// belongs to the fallback; a live pass goes to the server while it answers,
/// to the fallback while it does not, and nowhere if there is no fallback —
/// the overlay then simply shows no live text.
pub fn pass_route(config: &AppConfig, is_interim: bool, server_offline: bool) -> PassRoute {
    if config.engine.backend != BackendChoice::RemoteOpenAi || !is_interim {
        PassRoute::Local
    } else if !server_offline {
        PassRoute::Remote
    } else if is_standby_config(config) {
        PassRoute::Local
    } else {
        PassRoute::Skip
    }
}

/// A live pass is only useful while the user is still talking, so it gets far
/// less time than a whole dictation: one that has not come back by then is
/// stale, and waiting longer would hold up the passes behind it.
const LIVE_PASS_TIMEOUT_SECS: u64 = 6;

fn build_live_remote(config: &AppConfig) -> Option<Box<dyn TranscriptionBackend>> {
    (config.engine.backend == BackendChoice::RemoteOpenAi).then(|| {
        let mut remote = config.engine.remote_openai.clone();
        remote.timeout_secs = remote.timeout_secs.clamp(1, LIVE_PASS_TIMEOUT_SECS);
        Box::new(remote_openai::RemoteOpenAiBackend::new(remote)) as Box<dyn TranscriptionBackend>
    })
}

impl InferenceEngine {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let backend = build_backend(&config);
        let live_remote = build_live_remote(&config);
        Self { config, backend, live_remote }
    }

    /// Where a pass would go right now. See [`pass_route`].
    pub fn route(&self, is_interim: bool) -> PassRoute {
        pass_route(&self.config, is_interim, remote_status::is_offline())
    }

    /// Load the selected backend model. Blocks until ready.
    pub fn load(&mut self) -> Result<()> {
        self.backend.load()
    }

    pub fn unload(&mut self) {
        self.backend.unload();
    }

    /// Whether this engine is a remote server's fallback: built, but kept
    /// unloaded until the server cannot be reached. See [`is_standby_config`].
    pub fn is_standby(&self) -> bool {
        is_standby_config(&self.config)
    }

    /// Update engine configuration. If backend or backend model settings changed,
    /// re-creates the backend and returns `true` (meaning the caller should reload).
    pub fn update_config(&mut self, new_config: Arc<AppConfig>) -> bool {
        // What this engine actually runs is the on-device engine: the selected
        // backend, or a remote backend's fallback. Only a change to that (or to
        // its own settings) is a reason to rebuild it — editing the server's
        // address must not throw away a loaded fallback model.
        let old_local = local_engine_choice(&self.config);
        let new_local = local_engine_choice(&new_config);
        let backend_changed = self.config.engine.backend != new_config.engine.backend
            || old_local != new_local
            || match new_local {
                Some(BackendChoice::WhisperCpp) => self.config.engine.whisper_cpp != new_config.engine.whisper_cpp,
                Some(BackendChoice::Moonshine) => self.config.engine.moonshine != new_config.engine.moonshine,
                Some(BackendChoice::Parakeet) => self.config.engine.parakeet != new_config.engine.parakeet,
                // Remote with no fallback: the backend is the HTTP client itself.
                _ => self.config.engine.remote_openai != new_config.engine.remote_openai,
            };

        // The live-pass client follows the server's address and key, which the
        // check above deliberately ignores when a fallback is configured.
        if self.config.engine.backend != new_config.engine.backend
            || self.config.engine.remote_openai != new_config.engine.remote_openai
        {
            self.live_remote = build_live_remote(&new_config);
        }

        self.config = new_config.clone();

        if backend_changed {
            info!(
                "Inference backend configuration changed, switching backend to {:?}",
                new_config.engine.backend
            );
            self.backend.unload();
            self.backend = build_backend(&new_config);
            true
        } else {
            false
        }
    }

    /// Transcribe and post-process. Returns the final text.
    pub fn process(&self, req: InferenceRequest) -> Result<InferenceOutput> {
        let route = self.route(req.is_interim);
        self.process_routed(req, route)
    }

    /// [`process`](Self::process) with the route already decided, for a caller
    /// that had to know it first (the worker, to decide whether to load a model).
    pub fn process_routed(&self, req: InferenceRequest, route: PassRoute) -> Result<InferenceOutput> {
        let is_interim = req.is_interim;
        let session_id = req.session_id;

        if route == PassRoute::Skip {
            return Ok(InferenceOutput::failed(
                req.target_id,
                req.binding_id,
                &anyhow::anyhow!(NO_LIVE_ENGINE),
                is_interim,
                session_id,
            ));
        }

        // The in-memory config: no disk I/O on the hot path, and it is current —
        // `save_config` pushes every change here through `update_config`.
        let app_config = &*self.config;

        // ── Noise Gate (VAD) ──────────────────────────────────────────────────
        let rms = finalize::rms(&req.audio);
        let rms_threshold = finalize::noise_gate_threshold(app_config.audio.vad_threshold);
        if req.audio.is_empty() || rms < rms_threshold {
            if !req.audio.is_empty() {
                info!(
                    "Audio skipped by noise gate: RMS is {rms:.5} (threshold is {rms_threshold:.5}, vad_threshold={:.2})",
                    app_config.audio.vad_threshold
                );
            }
            return Ok(InferenceOutput::empty(req.target_id, req.binding_id, is_interim, session_id));
        }

        // Moonshine, Parakeet and Remote-OpenAI backends read their language
        // setting straight from their own config, ignoring this field; only
        // whisper.cpp consults it, treating "auto"/empty as auto-detect.
        let language: Option<String> = (local_engine_choice(app_config) == Some(BackendChoice::WhisperCpp))
            .then(|| app_config.engine.whisper_cpp.language.trim())
            .filter(|lang| !lang.is_empty() && *lang != "auto")
            .map(str::to_string);

        let t_req = TranscribeRequest {
            audio: req.audio,
            language,
            word_timestamps: false,
            initial_prompt: Some(finalize::initial_prompt(
                &app_config.features.custom_vocabulary,
                &app_config.features.snippets,
            )),
            interim_session: is_interim.then_some(session_id),
        };
        let remote = match (route, self.live_remote.as_deref()) {
            (PassRoute::Remote, Some(remote)) => Some(remote),
            _ => None,
        };
        let result = match remote.unwrap_or(&*self.backend).transcribe(&t_req) {
            Ok(r) => {
                if remote.is_some() {
                    // An answer is the freshest proof there is that the server is up.
                    remote_status::set_online();
                }
                r
            }
            // A live pass the server did not answer costs the overlay one
            // update and nothing else: the dictation itself is streamed to the
            // server separately. So this is not an error worth a log line per
            // pass — but a refused connection is worth knowing about at once,
            // rather than at the next probe.
            Err(e) if remote.is_some() => {
                if remote_openai::is_connect_failure(&e) {
                    remote_status::set_offline(remote_openai::failure_reason(&e));
                }
                tracing::debug!("Live pass to the remote server failed: {e:#}");
                return Ok(InferenceOutput::failed(req.target_id, req.binding_id, &e, is_interim, session_id));
            }
            // Stopped part-way because the recording ended: not a failure.
            Err(_) if is_interim && interim_cancelled(session_id) => {
                return Ok(InferenceOutput::failed(
                    req.target_id,
                    req.binding_id,
                    &anyhow::anyhow!(CANCELLED),
                    is_interim,
                    session_id,
                ));
            }
            Err(e) => return Err(e),
        };

        let mut processed = finalize::post_process(&result.text, rms, &req.processing, app_config);

        // ── Hotkey-Specific OpenAI Post-Processing ────────────────────────────
        // Interim passes only feed the voice-command trigger check and are
        // thrown away; running the hotkey's (billed, slow) OpenAI rewrite on
        // every one of them would cost a request per pass for nothing.
        if !is_interim && !processed.is_empty() {
            if let Some(openai_cfg) =
                finalize::binding_openai_config(req.binding.as_ref(), &app_config.openai)
            {
                processed = run_llm_rewrite(voxctrl_llm::OpenAiClient::new(openai_cfg), processed);
            }
        }

        Ok(InferenceOutput {
            text: processed,
            target_id: req.target_id,
            binding_id: req.binding_id,
            raw_text: result.text,
            inference_ms: result.inference_ms,
            language: result.language,
            error: None,
            is_interim,
            session_id,
        })
    }
}

/// Run the (async) LLM rewrite from this synchronous worker, returning `text`
/// unchanged if it cannot run.
fn run_llm_rewrite(client: voxctrl_llm::OpenAiClient, text: String) -> String {
    match tokio::runtime::Handle::try_current() {
        // Blocking on a runtime thread would stall it, so block on a fresh one.
        Ok(handle) => {
            let fallback = text.clone();
            std::thread::spawn(move || handle.block_on(async { client.process(&text).await }))
                .join()
                .unwrap_or(fallback)
        }
        Err(_) => match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt.block_on(async { client.process(&text).await }),
            Err(_) => text,
        },
    }
}

// ── Backend selection ─────────────────────────────────────────────────────────

/// Build the transcription backend `config.engine` selects, not yet loaded.
///
/// Public for `voxctrl-server`, which drives a backend directly: it serves raw
/// transcripts and leaves post-processing to the client that asked for them.
pub fn build_backend(config: &AppConfig) -> Box<dyn TranscriptionBackend> {
    // A remote backend with a fallback configured builds the fallback: the
    // recording pipeline talks to the server itself, and this engine is only
    // handed a dictation when the server could not take it.
    let choice = local_engine_choice(config).unwrap_or(BackendChoice::RemoteOpenAi);
    build_backend_for(&choice, config)
}

/// The on-device engine `config` transcribes with: the selected backend, or
/// the fallback of a remote one. `None` when a remote backend has no fallback
/// and nothing runs locally at all.
pub fn local_engine_choice(config: &AppConfig) -> Option<BackendChoice> {
    match config.engine.backend {
        BackendChoice::RemoteOpenAi => config.engine.remote_openai.fallback.backend(),
        ref local => Some(local.clone()),
    }
}

/// Whether `config` keeps an on-device engine in reserve behind a remote
/// server. Such an engine is not loaded at startup: it costs memory only
/// while the server is unreachable.
pub fn is_standby_config(config: &AppConfig) -> bool {
    config.engine.backend == BackendChoice::RemoteOpenAi
        && config.engine.remote_openai.fallback.backend().is_some()
}

fn build_backend_for(choice: &BackendChoice, config: &AppConfig) -> Box<dyn TranscriptionBackend> {
    match choice {
        BackendChoice::WhisperCpp => {
            Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
        }
        BackendChoice::Moonshine => {
            #[cfg(feature = "moonshine")]
            {
                info!(
                    "Using Moonshine backend ({} model)",
                    config.engine.moonshine.model_size
                );
                Box::new(moonshine::MoonshineBackend::new(config.engine.moonshine.clone()))
            }
            #[cfg(not(feature = "moonshine"))]
            {
                // Moonshine feature not compiled — fall back to whisper-cpp.
                tracing::warn!("Moonshine backend selected but not compiled in this build; using whisper-cpp");
                Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
            }
        }
        BackendChoice::Parakeet => {
            #[cfg(feature = "parakeet")]
            {
                info!(
                    "Using Parakeet backend ({} model)",
                    config.engine.parakeet.model_size
                );
                Box::new(parakeet::ParakeetBackend::new(config.engine.parakeet.clone()))
            }
            #[cfg(not(feature = "parakeet"))]
            {
                // Parakeet feature not compiled — fall back to whisper-cpp.
                tracing::warn!("Parakeet backend selected but not compiled in this build; using whisper-cpp");
                Box::new(WhisperCppBackend::new(config.engine.whisper_cpp.clone()))
            }
        }
        BackendChoice::RemoteOpenAi => {
            info!(
                "Using Remote OpenAI speech engine ({})",
                config.engine.remote_openai.endpoint
            );
            Box::new(remote_openai::RemoteOpenAiBackend::new(
                config.engine.remote_openai.clone(),
            ))
        }
    }
}

// ── Threaded worker ───────────────────────────────────────────────────────────

/// How often the worker checks whether a loaded fallback model can go.
const STANDBY_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

/// How long a loaded fallback model is kept without being used while the
/// server is still unreachable. Long enough that dictating every few minutes
/// through an outage does not reload it each time.
const STANDBY_IDLE_UNLOAD: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Whether a loaded fallback model should be unloaded: as soon as the server
/// answers again, or after sitting unused for [`STANDBY_IDLE_UNLOAD`].
fn standby_should_unload(server_online: bool, idle: std::time::Duration) -> bool {
    server_online || idle >= STANDBY_IDLE_UNLOAD
}

/// Run the inference engine on a dedicated OS thread.
/// Receives `InferenceRequest` from `rx`, sends `InferenceOutput` to `tx`.
pub fn run_worker(
    config: Arc<AppConfig>,
    rx: Receiver<InferenceRequest>,
    tx: Sender<InferenceOutput>,
) {
    let (_dummy_tx, dummy_rx) = crossbeam_channel::unbounded();
    run_worker_with_config(config, rx, tx, dummy_rx);
}

/// Run the inference engine on a dedicated OS thread with dynamic config reloading.
/// Receives `InferenceRequest` from `rx`, sends `InferenceOutput` to `tx`,
/// and updates/reloads the backend whenever `config_rx` receives a new `AppConfig`.
pub fn run_worker_with_config(
    config: Arc<AppConfig>,
    rx: Receiver<InferenceRequest>,
    tx: Sender<InferenceOutput>,
    config_rx: Receiver<Arc<AppConfig>>,
) {
    std::thread::Builder::new()
        .name("voxctrl-inference".into())
        .spawn(move || {
            let mut engine = InferenceEngine::new(config);
            // Checked every few seconds to unload a fallback model the app no
            // longer needs. See the `ticker` arm below.
            let ticker = crossbeam_channel::tick(STANDBY_CHECK_INTERVAL);
            let mut last_used = std::time::Instant::now();
            // A load failure (typically: model not downloaded yet on a fresh
            // install) must NOT kill this thread. Keep consuming requests and
            // retry the load on each one — the user may download the model
            // from Settings → Engine while the app is running, and dictation
            // must start working right away, not after an app restart.
            //
            // A remote server's fallback is deliberately not loaded here: it is
            // loaded by the first dictation the server could not take.
            let mut loaded = if engine.is_standby() {
                info!("Fallback speech engine on standby (not loaded)");
                false
            } else {
                match engine.load() {
                    Ok(()) => {
                        info!("Inference engine ready");
                        true
                    }
                    Err(e) => {
                        error!("Failed to load inference backend: {e:#}");
                        false
                    }
                }
            };

            loop {
                crossbeam_channel::select! {
                    recv(rx) -> req_res => {
                        let mut req = match req_res {
                            Ok(r) => r,
                            Err(_) => break,
                        };

                        // If an interim request arrived, drain any newer requests already in the queue
                        // to prioritize fresher audio or the final release request.
                        if req.is_interim {
                            while let Ok(newer) = rx.try_recv() {
                                req = newer;
                                if !req.is_interim {
                                    break;
                                }
                            }
                        }

                        let is_interim = req.is_interim;
                        let session_id = req.session_id;

                        // A live pass for a recording that has already ended
                        // is not started at all: the final pass is next.
                        if is_interim && interim_cancelled(session_id) {
                            let _ = tx.send(InferenceOutput::failed(
                                req.target_id,
                                req.binding_id,
                                &anyhow::anyhow!(CANCELLED),
                                is_interim,
                                session_id,
                            ));
                            continue;
                        }

                        // A pass bound for the server needs no local model: a
                        // fallback on standby must stay unloaded for it.
                        let route = engine.route(is_interim);
                        if !loaded && route == PassRoute::Local {
                            match engine.load() {
                                Ok(()) => {
                                    info!("Inference engine ready (loaded on demand)");
                                    loaded = true;
                                }
                                Err(e) => {
                                    error!("Inference backend still not loadable: {e:#}");
                                    let _ = tx.send(InferenceOutput::failed(
                                        req.target_id, req.binding_id, &e, is_interim, session_id,
                                    ));
                                    continue;
                                }
                            }
                        }

                        // Kept for the error report, which would otherwise lose
                        // which target and hotkey the failed utterance was for.
                        let (target_id, binding_id) = (req.target_id.clone(), req.binding_id.clone());
                        let output = engine.process_routed(req, route).unwrap_or_else(|e| {
                            error!("Inference error: {e:?}");
                            InferenceOutput::failed(target_id, binding_id, &e, is_interim, session_id)
                        });
                        if route == PassRoute::Local {
                            last_used = std::time::Instant::now();
                        }
                        let _ = tx.send(output);
                    }
                    recv(ticker) -> _ => {
                        if loaded
                            && engine.is_standby()
                            && standby_should_unload(remote_status::is_online(), last_used.elapsed())
                        {
                            info!("Unloading the fallback speech engine: the server is reachable again, or it has been idle");
                            engine.unload();
                            loaded = false;
                        }
                    }
                    recv(config_rx) -> new_cfg_res => {
                        let new_cfg = match new_cfg_res {
                            Ok(c) => c,
                            Err(_) => break,
                        };
                        let needs_reload = engine.update_config(new_cfg);
                        if needs_reload && engine.is_standby() {
                            // A new fallback: built, and left unloaded until needed.
                            loaded = false;
                        } else if needs_reload {
                            loaded = match engine.load() {
                                Ok(()) => {
                                    info!("Inference engine ready with new backend");
                                    true
                                }
                                Err(e) => {
                                    error!("Failed to load new inference backend: {e:#}");
                                    false
                                }
                            };
                        }
                    }
                }
            }
        })
        .expect("failed to spawn inference thread");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that records every request it is given and answers with a
    /// fixed transcript, so `process` can be driven end to end without a model.
    struct RecordingBackend {
        transcript: &'static str,
        requests: Arc<std::sync::Mutex<Vec<TranscribeRequest>>>,
    }

    impl TranscriptionBackend for RecordingBackend {
        fn name(&self) -> &str {
            "recording"
        }
        fn load(&mut self) -> Result<()> {
            Ok(())
        }
        fn transcribe(&self, req: &TranscribeRequest) -> Result<backend::TranscriptionResult> {
            self.requests.lock().unwrap().push(req.clone());
            Ok(backend::TranscriptionResult {
                text: self.transcript.to_string(),
                language: "en".into(),
                language_probability: 1.0,
                duration_ms: 0,
                inference_ms: 1,
                word_timestamps: None,
            })
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
    }

    fn recording_engine(
        cfg: AppConfig,
        transcript: &'static str,
    ) -> (InferenceEngine, Arc<std::sync::Mutex<Vec<TranscribeRequest>>>) {
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let engine = InferenceEngine {
            config: Arc::new(cfg),
            backend: Box::new(RecordingBackend { transcript, requests: requests.clone() }),
            live_remote: None,
        };
        (engine, requests)
    }

    fn remote_config(fallback: voxctrl_config::FallbackChoice) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.engine.backend = BackendChoice::RemoteOpenAi;
        cfg.engine.remote_openai.fallback = fallback;
        cfg
    }

    #[test]
    fn the_local_engine_is_the_backend_or_a_remote_backends_fallback() {
        use voxctrl_config::FallbackChoice;
        let mut local = AppConfig::default();
        local.engine.backend = BackendChoice::Parakeet;
        assert_eq!(local_engine_choice(&local), Some(BackendChoice::Parakeet));
        assert!(!is_standby_config(&local));

        let bare = remote_config(FallbackChoice::None);
        assert_eq!(local_engine_choice(&bare), None);
        assert!(!is_standby_config(&bare));

        let backed = remote_config(FallbackChoice::Parakeet);
        assert_eq!(local_engine_choice(&backed), Some(BackendChoice::Parakeet));
        assert!(is_standby_config(&backed));
    }

    #[test]
    fn a_remote_backend_builds_its_fallback_engine_or_the_http_client() {
        use voxctrl_config::FallbackChoice;
        assert_eq!(build_backend(&remote_config(FallbackChoice::None)).name(), "remote-openai");
        assert_eq!(build_backend(&remote_config(FallbackChoice::WhisperCpp)).name(), "whisper-cpp");
    }

    #[test]
    fn a_fallback_model_is_unloaded_when_the_server_returns_or_it_sits_idle() {
        let short = std::time::Duration::from_secs(30);
        assert!(!standby_should_unload(false, short), "still offline and in use: keep it");
        assert!(standby_should_unload(true, short), "the server is back");
        assert!(standby_should_unload(false, STANDBY_IDLE_UNLOAD), "idle through a long outage");
    }

    /// Only a change to what actually runs on this machine rebuilds the
    /// engine. Editing the server's address or key must not throw away a
    /// fallback model that is loaded and in use.
    #[test]
    fn remote_settings_rebuild_the_engine_only_when_the_fallback_changes() {
        use voxctrl_config::FallbackChoice;
        let (mut engine, _) = recording_engine(remote_config(FallbackChoice::WhisperCpp), "x");
        assert!(engine.is_standby());

        let mut moved = remote_config(FallbackChoice::WhisperCpp);
        moved.engine.remote_openai.endpoint = "http://other-server:8787/v1".into();
        moved.engine.remote_openai.api_key = Some("vx_new".into());
        assert!(!engine.update_config(Arc::new(moved.clone())), "a new address rebuilt the fallback");

        let mut bigger = moved.clone();
        bigger.engine.whisper_cpp.model_size = "small".into();
        assert!(engine.update_config(Arc::new(bigger.clone())), "the fallback's own model changed");

        // Another engine's settings are not this fallback's business.
        let mut unrelated = bigger.clone();
        unrelated.engine.moonshine.model_size = "tiny".into();
        assert!(!engine.update_config(Arc::new(unrelated.clone())));

        let mut dropped = unrelated.clone();
        dropped.engine.remote_openai.fallback = FallbackChoice::None;
        assert!(engine.update_config(Arc::new(dropped)), "the fallback was removed");
        assert!(!engine.is_standby());
    }

    /// With no fallback the engine is the HTTP client, which does follow the
    /// server's address.
    #[test]
    fn remote_settings_without_a_fallback_rebuild_the_client() {
        use voxctrl_config::FallbackChoice;
        let (mut engine, _) = recording_engine(remote_config(FallbackChoice::None), "x");
        let mut moved = remote_config(FallbackChoice::None);
        moved.engine.remote_openai.endpoint = "http://other-server:8787/v1".into();
        assert!(engine.update_config(Arc::new(moved)));
    }

    /// A whisper.cpp fallback is given the language from its own settings,
    /// exactly as when whisper.cpp is the selected backend.
    #[test]
    fn a_whisper_fallback_transcribes_in_its_configured_language() {
        let mut cfg = remote_config(voxctrl_config::FallbackChoice::WhisperCpp);
        cfg.engine.whisper_cpp.language = "de".into();
        let (engine, requests) = recording_engine(cfg, "hallo");
        engine.process(request(speech())).unwrap();
        assert_eq!(requests.lock().unwrap()[0].language.as_deref(), Some("de"));

        let mut cfg = remote_config(voxctrl_config::FallbackChoice::Parakeet);
        cfg.engine.whisper_cpp.language = "de".into();
        let (engine, requests) = recording_engine(cfg, "hello");
        engine.process(request(speech())).unwrap();
        assert_eq!(requests.lock().unwrap()[0].language, None);
    }

    #[test]
    fn where_each_pass_goes() {
        use voxctrl_config::FallbackChoice;
        let local = AppConfig::default();
        let bare = remote_config(FallbackChoice::None);
        let backed = remote_config(FallbackChoice::Parakeet);

        // A local backend does everything, whatever a server elsewhere is doing.
        for offline in [false, true] {
            assert_eq!(pass_route(&local, true, offline), PassRoute::Local);
            assert_eq!(pass_route(&local, false, offline), PassRoute::Local);
        }
        // Live passes go to the server while it answers.
        assert_eq!(pass_route(&bare, true, false), PassRoute::Remote);
        assert_eq!(pass_route(&backed, true, false), PassRoute::Remote);
        // With the server down they go to the fallback, or nowhere.
        assert_eq!(pass_route(&backed, true, true), PassRoute::Local);
        assert_eq!(pass_route(&bare, true, true), PassRoute::Skip);
        // A final pass only reaches this engine as a fallback request.
        assert_eq!(pass_route(&backed, false, false), PassRoute::Local);
        assert_eq!(pass_route(&backed, false, true), PassRoute::Local);
    }

    /// An engine with a remote backend, a recording local backend and a
    /// recording stand-in for the server.
    #[allow(clippy::type_complexity)]
    fn remote_engine(
        fallback: voxctrl_config::FallbackChoice,
    ) -> (
        InferenceEngine,
        Arc<std::sync::Mutex<Vec<TranscribeRequest>>>,
        Arc<std::sync::Mutex<Vec<TranscribeRequest>>>,
    ) {
        let local = Arc::new(std::sync::Mutex::new(Vec::new()));
        let server = Arc::new(std::sync::Mutex::new(Vec::new()));
        let engine = InferenceEngine {
            config: Arc::new(remote_config(fallback)),
            backend: Box::new(RecordingBackend { transcript: "from the fallback", requests: local.clone() }),
            live_remote: Some(Box::new(RecordingBackend { transcript: "from the server", requests: server.clone() })),
        };
        (engine, local, server)
    }

    #[test]
    fn a_live_pass_routed_to_the_server_is_transcribed_there() {
        // A pass the server answers marks it online, which is process-wide.
        let _guard = remote_status::TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (engine, local, server) = remote_engine(voxctrl_config::FallbackChoice::Parakeet);
        let live = InferenceRequest { is_interim: true, session_id: 9_100, ..request(speech()) };
        let out = engine.process_routed(live, PassRoute::Remote).unwrap();
        assert_eq!(out.raw_text, "from the server");
        assert!(out.is_interim);
        assert_eq!(server.lock().unwrap().len(), 1);
        assert!(local.lock().unwrap().is_empty(), "the fallback was used for a live pass the server could take");
        assert!(remote_status::is_online(), "an answered pass is proof the server is up");
        remote_status::set_not_in_use();
    }

    #[test]
    fn a_pass_routed_locally_uses_the_fallback() {
        let (engine, local, server) = remote_engine(voxctrl_config::FallbackChoice::Parakeet);
        let out = engine.process_routed(request(speech()), PassRoute::Local).unwrap();
        assert_eq!(out.raw_text, "from the fallback");
        assert_eq!(local.lock().unwrap().len(), 1);
        assert!(server.lock().unwrap().is_empty());
    }

    #[test]
    fn a_skipped_live_pass_runs_nothing_and_reports_no_text() {
        let (engine, local, server) = remote_engine(voxctrl_config::FallbackChoice::None);
        let live = InferenceRequest { is_interim: true, session_id: 9_101, ..request(speech()) };
        let out = engine.process_routed(live, PassRoute::Skip).unwrap();
        assert!(out.error.is_some());
        assert!(out.text.is_empty());
        assert!(local.lock().unwrap().is_empty() && server.lock().unwrap().is_empty());
    }

    #[test]
    fn the_live_client_exists_only_for_a_remote_backend_and_follows_its_address() {
        use voxctrl_config::FallbackChoice;
        assert!(InferenceEngine::new(Arc::new(AppConfig::default())).live_remote.is_none());

        let mut engine = InferenceEngine::new(Arc::new(remote_config(FallbackChoice::WhisperCpp)));
        assert_eq!(engine.live_remote.as_ref().map(|b| b.name()), Some("remote-openai"));

        // Back to a local backend: there is no server to send live passes to.
        engine.update_config(Arc::new(AppConfig::default()));
        assert!(engine.live_remote.is_none());
    }

    #[test]
    fn a_live_pass_gets_a_short_timeout_however_long_dictations_are_allowed() {
        let mut cfg = remote_config(voxctrl_config::FallbackChoice::None);
        cfg.engine.remote_openai.timeout_secs = 120;
        assert!(build_live_remote(&cfg).is_some());
        // The clamp itself, since the client does not expose its timeout.
        assert_eq!(120u64.clamp(1, LIVE_PASS_TIMEOUT_SECS), LIVE_PASS_TIMEOUT_SECS);
        assert_eq!(3u64.clamp(1, LIVE_PASS_TIMEOUT_SECS), 3);
    }

    /// A fallback engine is left unloaded at startup and loaded by the first
    /// dictation handed to it. With no model on disk that load fails, and the
    /// failure reaches the user as that dictation's error, naming the model.
    #[test]
    fn the_worker_loads_a_fallback_only_when_a_dictation_needs_it() {
        let models = tempfile::tempdir().unwrap();
        let mut cfg = remote_config(voxctrl_config::FallbackChoice::WhisperCpp);
        cfg.engine.whisper_cpp.model_dir = models.path().to_string_lossy().into_owned();

        let (req_tx, req_rx) = crossbeam_channel::unbounded();
        let (out_tx, out_rx) = crossbeam_channel::unbounded();
        // Held for the test: the worker stops when its config channel closes.
        let (_config_tx, config_rx) = crossbeam_channel::unbounded();
        run_worker_with_config(Arc::new(cfg), req_rx, out_tx, config_rx);

        // Nothing was loaded (or attempted) at startup: no output until asked.
        assert!(out_rx.recv_timeout(std::time::Duration::from_millis(300)).is_err());

        req_tx.send(request(speech())).unwrap();
        let out = out_rx.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        let error = out.error.expect("there is no model to load");
        assert!(error.contains("not downloaded"), "{error}");
        assert_eq!(out.target_id, "notes");
    }

    /// Loud enough to pass the noise gate at any sensitivity.
    fn speech() -> Vec<f32> {
        vec![0.1; 1600]
    }

    fn request(audio: Vec<f32>) -> InferenceRequest {
        InferenceRequest {
            audio,
            target_id: "notes".into(),
            binding_id: None,
            is_interim: false,
            session_id: 7,
            processing: Default::default(),
            binding: None,
        }
    }

    /// A backend that stops part-way the way whisper.cpp does once a live
    /// pass is cancelled: it reports an error.
    struct AbortingBackend;

    impl TranscriptionBackend for AbortingBackend {
        fn name(&self) -> &str {
            "aborting"
        }
        fn load(&mut self) -> Result<()> {
            Ok(())
        }
        fn transcribe(&self, req: &TranscribeRequest) -> Result<backend::TranscriptionResult> {
            match req.interim_session {
                Some(s) if interim_cancelled(s) => anyhow::bail!("whisper full: aborted"),
                _ => Ok(backend::TranscriptionResult {
                    text: "final words".into(),
                    language: "en".into(),
                    language_probability: 1.0,
                    duration_ms: 0,
                    inference_ms: 1,
                    word_timestamps: None,
                }),
            }
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
    }

    /// Releasing the key ends that recording's live passes, and those of
    /// earlier recordings, but not a later recording's — and never the final
    /// pass. A pass stopped part-way is reported as producing nothing, not as
    /// a transcription failure. (The only test in this crate that touches the
    /// process-wide switch, so it cannot disturb the others.)
    #[test]
    fn ending_a_recording_cancels_its_live_passes_but_not_the_final_one() {
        cancel_interim_passes(41);
        assert!(interim_cancelled(40));
        assert!(interim_cancelled(41));
        assert!(!interim_cancelled(42), "a newer recording's passes must keep running");
        cancel_interim_passes(3); // an older id never moves it back
        assert!(interim_cancelled(41));

        let engine = InferenceEngine {
            config: Arc::new(AppConfig::default()),
            backend: Box::new(AbortingBackend),
            live_remote: None,
        };
        let live = |session| InferenceRequest { is_interim: true, session_id: session, ..request(speech()) };

        let stopped = engine.process(live(41)).expect("a cancelled pass is not an error");
        assert!(stopped.text.is_empty());
        assert!(stopped.error.as_deref().is_some_and(|e| e.contains("cancelled")));

        let running = engine.process(live(42)).unwrap();
        assert_eq!(running.text, "final words");

        // The final pass for the very recording that was cancelled still runs.
        let fin = engine.process(InferenceRequest { session_id: 41, ..request(speech()) }).unwrap();
        assert!(fin.error.is_none());
        assert_eq!(fin.text, "final words");
    }

    /// Live passes tell the backend which recording they belong to, so it can
    /// stop once that recording ends; the final pass does not.
    #[test]
    fn only_live_passes_carry_their_session_to_the_backend() {
        let (engine, requests) = recording_engine(AppConfig::default(), "hello");
        engine.process(InferenceRequest { is_interim: true, session_id: 9_000, ..request(speech()) }).unwrap();
        engine.process(request(speech())).unwrap();
        let seen = requests.lock().unwrap();
        assert_eq!(seen[0].interim_session, Some(9_000));
        assert_eq!(seen[1].interim_session, None);
    }

    /// Only whisper.cpp takes a language hint, and only from its own setting:
    /// the Moonshine language (a regression once read it whenever the device
    /// was not "auto") must never reach it, whatever the device.
    #[test]
    fn whisper_gets_only_its_own_language_setting() {
        for device in ["auto", "cpu", "cuda", "vulkan"] {
            let mut cfg = AppConfig::default();
            cfg.engine.whisper_cpp.device = device.to_string();
            cfg.engine.moonshine.language = "fr".to_string();

            for (whisper_lang, expected) in [("auto", None), ("", None), (" de ", Some("de"))] {
                cfg.engine.whisper_cpp.language = whisper_lang.to_string();
                let (engine, requests) = recording_engine(cfg.clone(), "hello");
                engine.process(request(speech())).unwrap();
                assert_eq!(
                    requests.lock().unwrap()[0].language.as_deref(),
                    expected,
                    "device {device}, whisper language {whisper_lang:?}"
                );
            }
        }

        // Other backends read their language from their own config.
        let mut cfg = AppConfig::default();
        cfg.engine.backend = BackendChoice::Moonshine;
        cfg.engine.whisper_cpp.language = "de".to_string();
        let (engine, requests) = recording_engine(cfg, "hello");
        engine.process(request(speech())).unwrap();
        assert_eq!(requests.lock().unwrap()[0].language, None);
    }

    /// `process` works from the engine's in-memory config — no config.json
    /// read per utterance — and a config pushed through `update_config` (what
    /// `save_config` does on every save) applies to the very next utterance.
    #[test]
    fn process_uses_the_engine_config_and_follows_updates() {
        let mut cfg = AppConfig::default();
        cfg.features.remove_fillers = true;
        cfg.features.custom_vocabulary = vec!["Kubernetes".into()];
        let (mut engine, requests) = recording_engine(cfg.clone(), "um hello world");

        let first = engine.process(request(speech())).unwrap();
        assert!(!first.text.to_lowercase().contains("um"), "fillers kept: {:?}", first.text);
        assert_eq!(first.raw_text, "um hello world");
        assert_eq!(first.session_id, 7);
        let prompt = requests.lock().unwrap()[0].initial_prompt.clone().unwrap();
        assert!(prompt.contains("Kubernetes"), "vocabulary missing from prompt: {prompt}");

        cfg.features.remove_fillers = false;
        assert!(!engine.update_config(Arc::new(cfg)), "a features change rebuilt the backend");
        let second = engine.process(request(speech())).unwrap();
        assert!(second.text.to_lowercase().contains("um"), "update ignored: {:?}", second.text);
    }

    /// The target's overrides arrive with the request and win over the
    /// global settings — the engine never looks them up on disk.
    #[test]
    fn the_request_carries_the_target_overrides() {
        let mut cfg = AppConfig::default();
        cfg.features.remove_fillers = true;
        let (engine, _) = recording_engine(cfg, "um hello world");

        let mut req = request(speech());
        req.processing.remove_fillers = Some(false);
        let out = engine.process(req).unwrap();
        assert!(out.text.to_lowercase().contains("um"), "override ignored: {:?}", out.text);
    }

    #[test]
    fn silence_is_gated_before_the_backend_runs() {
        let (engine, requests) = recording_engine(AppConfig::default(), "Thank you.");
        let out = engine.process(request(vec![0.0; 1600])).unwrap();
        assert!(out.text.is_empty() && out.error.is_none());
        assert!(requests.lock().unwrap().is_empty(), "silent audio reached the backend");
    }

    #[test]
    fn default_backend_is_whisper_cpp() {
        let cfg = AppConfig::default();
        assert_eq!(build_backend(&cfg).name(), "whisper-cpp");
    }

    #[test]
    fn remote_openai_backend_builds_correctly() {
        let mut cfg = AppConfig::default();
        cfg.engine.backend = BackendChoice::RemoteOpenAi;
        cfg.engine.remote_openai.endpoint = "http://192.168.1.100:8000/v1".to_string();
        let backend = build_backend(&cfg);
        assert_eq!(backend.name(), "remote-openai");
        assert!(backend.is_loaded());
    }

    #[test]
    fn test_engine_update_config_switches_backend() {
        let cfg = AppConfig::default();
        let mut engine = InferenceEngine::new(Arc::new(cfg.clone()));
        assert_eq!(engine.backend.name(), "whisper-cpp");

        let mut new_cfg = cfg.clone();
        new_cfg.engine.backend = BackendChoice::RemoteOpenAi;
        new_cfg.engine.remote_openai.endpoint = "http://localhost:5000/v1".to_string();
        let reloaded = engine.update_config(Arc::new(new_cfg));
        assert!(reloaded);
        assert_eq!(engine.backend.name(), "remote-openai");

        // Non-backend config change should not trigger backend reload
        let mut features_cfg = engine.config.as_ref().clone();
        features_cfg.features.remove_fillers = !features_cfg.features.remove_fillers;
        let reloaded_features = engine.update_config(Arc::new(features_cfg));
        assert!(!reloaded_features);
        assert_eq!(engine.backend.name(), "remote-openai");
    }
}
