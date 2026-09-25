pub mod backend;
pub mod finalize;
#[cfg(feature = "moonshine")]
pub mod moonshine;
#[cfg(feature = "parakeet")]
pub mod parakeet;
pub mod postprocess;
pub mod remote_openai;
pub mod s1_mini;
mod util;
pub mod whisper_cpp;

pub use remote_openai::{
    test_remote_speech_engine, RemoteOpenAiBackend, RemoteSttTestResult, RemoteStreamingSession,
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
    if crate::s1_mini::sidecar_available() || cfg!(feature = "vulkan") {
        Some("vulkan")
    } else {
        None
    }
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
}

impl InferenceEngine {
    pub fn new(config: Arc<AppConfig>) -> Self {
        let backend = build_backend(&config);
        Self { config, backend }
    }

    /// Load the selected backend model. Blocks until ready.
    pub fn load(&mut self) -> Result<()> {
        self.backend.load()
    }

    pub fn unload(&mut self) {
        self.backend.unload();
    }

    /// Update engine configuration. If backend or backend model settings changed,
    /// re-creates the backend and returns `true` (meaning the caller should reload).
    pub fn update_config(&mut self, new_config: Arc<AppConfig>) -> bool {
        let backend_changed = self.config.engine.backend != new_config.engine.backend
            || (new_config.engine.backend == BackendChoice::WhisperCpp
                && self.config.engine.whisper_cpp != new_config.engine.whisper_cpp)
            || (new_config.engine.backend == BackendChoice::Moonshine
                && self.config.engine.moonshine != new_config.engine.moonshine)
            || (new_config.engine.backend == BackendChoice::Parakeet
                && self.config.engine.parakeet != new_config.engine.parakeet)
            || (new_config.engine.backend == BackendChoice::RemoteOpenAi
                && self.config.engine.remote_openai != new_config.engine.remote_openai);

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
        let is_interim = req.is_interim;
        let session_id = req.session_id;

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
        let language: Option<String> = (app_config.engine.backend == BackendChoice::WhisperCpp)
            .then(|| app_config.engine.whisper_cpp.language.trim())
            .filter(|lang| !lang.is_empty() && *lang != "auto")
            .map(str::to_string);

        let t_req = TranscribeRequest {
            audio: req.audio,
            language,
            word_timestamps: false,
            initial_prompt: Some(finalize::initial_prompt(&app_config.features.custom_vocabulary)),
        };
        let result = self.backend.transcribe(&t_req)?;

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

fn build_backend(config: &AppConfig) -> Box<dyn TranscriptionBackend> {
    match config.engine.backend {
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
            // A load failure (typically: model not downloaded yet on a fresh
            // install) must NOT kill this thread. Keep consuming requests and
            // retry the load on each one — the user may download the model
            // from Settings → Engine while the app is running, and dictation
            // must start working right away, not after an app restart.
            let mut loaded = match engine.load() {
                Ok(()) => {
                    info!("Inference engine ready");
                    true
                }
                Err(e) => {
                    error!("Failed to load inference backend: {e:#}");
                    false
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

                        if !loaded {
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
                        let output = engine.process(req).unwrap_or_else(|e| {
                            error!("Inference error: {e:?}");
                            InferenceOutput::failed(target_id, binding_id, &e, is_interim, session_id)
                        });
                        let _ = tx.send(output);
                    }
                    recv(config_rx) -> new_cfg_res => {
                        let new_cfg = match new_cfg_res {
                            Ok(c) => c,
                            Err(_) => break,
                        };
                        let needs_reload = engine.update_config(new_cfg);
                        if needs_reload {
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
        };
        (engine, requests)
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
