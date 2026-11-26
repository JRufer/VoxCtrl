//! The one speech model this server runs: fetching it, loading it, and
//! transcribing with it one request at a time.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex};

use anyhow::{bail, Context, Result};
use tracing::info;
use voxctrl_config::BackendChoice;
use voxctrl_inference::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

use crate::config::ServerConfig;

/// Below this level a clip is digital silence or close to it, and the model is
/// not run at all. Far under the desktop app's own noise gate (0.003 at its
/// default sensitivity), so it only catches what no model could transcribe —
/// including the 0.2 s of zeros the app's "Test connection" button sends.
const SILENCE_RMS: f32 = 1e-4;

/// What the server is running, for `/v1/models`, `/health` and the logs.
#[derive(Debug, Clone, PartialEq)]
pub struct EngineInfo {
    /// The backend's config name: `whisper-cpp`, `moonshine` or `parakeet`.
    pub backend: String,
    /// The model within it, e.g. `large-v3-turbo` or `tdt-0.6b-v3`.
    pub model: String,
    /// The configured language, or `auto`.
    pub language: String,
}

impl EngineInfo {
    pub fn from_config(cfg: &ServerConfig) -> Self {
        let e = &cfg.engine;
        let (backend, model, language) = match e.backend {
            BackendChoice::WhisperCpp => ("whisper-cpp", &e.whisper_cpp.model_size, &e.whisper_cpp.language),
            BackendChoice::Moonshine => ("moonshine", &e.moonshine.model_size, &e.moonshine.language),
            BackendChoice::Parakeet => ("parakeet", &e.parakeet.model_size, &e.parakeet.language),
            // `ServerConfig::validate` refuses this before an engine is built.
            BackendChoice::RemoteOpenAi => ("remote-openai", &e.whisper_cpp.model_size, &e.whisper_cpp.language),
        };
        Self { backend: backend.into(), model: model_name(model), language: language.clone() }
    }

    /// The id listed by `/v1/models`: `<backend>/<model>`.
    pub fn model_id(&self) -> String {
        format!("{}/{}", self.backend, self.model)
    }
}

/// Most dictations that may wait for the model at once. Past this the server
/// answers 503 rather than let the queue, and every client's wait, grow
/// without end: a dictation that would start minutes from now is better
/// handed to the client's fallback engine straight away.
pub const MAX_WAITING_DICTATIONS: usize = 8;

/// What a transcription is for, which decides how it waits for the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// Someone has finished speaking and is waiting for the text. Queued.
    Dictation,
    /// A live-text pass over a recording still in progress. Its result is
    /// replaced by the next pass, so it is never worth a dictation's wait:
    /// it runs only when the model is free and nothing is queued.
    Live,
}

/// Why a transcription did not run.
#[derive(Debug)]
pub enum TranscribeError {
    /// The model is busy: with anything (a live pass), or with a full queue
    /// (a dictation). Nothing was run; the client may try again.
    Busy(Purpose),
    /// The model ran and failed.
    Failed(anyhow::Error),
}

impl std::fmt::Display for TranscribeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Busy(Purpose::Live) => f.write_str("busy: live pass skipped"),
            Self::Busy(Purpose::Dictation) => f.write_str("busy: dictation queue full"),
            Self::Failed(e) => write!(f, "{e:#}"),
        }
    }
}

/// Who has the model, and how many dictations are waiting for it.
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
    freed: Condvar,
}

#[derive(Default)]
struct GateState {
    busy: bool,
    waiting: usize,
}

/// The model, held: dropping it hands the model to the next dictation.
struct Turn<'a>(&'a Gate);

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        self.0.state.lock().unwrap_or_else(|e| e.into_inner()).busy = false;
        self.0.freed.notify_one();
    }
}

impl Gate {
    fn enter(&self, purpose: Purpose, max_waiting: usize) -> Result<Turn<'_>, TranscribeError> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        match purpose {
            Purpose::Live => {
                if state.busy || state.waiting > 0 {
                    return Err(TranscribeError::Busy(Purpose::Live));
                }
            }
            Purpose::Dictation => {
                if state.busy || state.waiting > 0 {
                    if state.waiting >= max_waiting {
                        return Err(TranscribeError::Busy(Purpose::Dictation));
                    }
                    state.waiting += 1;
                    while state.busy {
                        state = self.freed.wait(state).unwrap_or_else(|e| e.into_inner());
                    }
                    state.waiting -= 1;
                }
            }
        }
        state.busy = true;
        Ok(Turn(self))
    }
}

/// Numbers live passes across the whole process, so each can be told to stop
/// on its own (see [`Engine::live_ticket`]). Process-wide because the record
/// of cancelled passes the backends poll is.
static LIVE_TICKETS: AtomicU64 = AtomicU64::new(voxctrl_inference::FIRST_SINGLE_PASS);

pub struct Engine {
    /// One transcription at a time, dictations first: see [`Gate::enter`].
    /// The backends hold a single inference state, and two transcriptions
    /// sharing one GPU would only take turns anyway.
    gate: Gate,
    max_waiting: usize,
    /// The ticket of the live pass that has the model now, or 0.
    running_live: AtomicU64,
    /// The newest live pass a dictation told to stop, so that pass's error
    /// is reported as "busy" and not as a failure of the model.
    preempted_live: AtomicU64,
    backend: Mutex<Box<dyn TranscriptionBackend>>,
    info: EngineInfo,
    /// whisper.cpp takes its language per request; the ONNX backends read
    /// theirs from their own config when they are built.
    whisper_language: Option<String>,
}

impl Engine {
    /// Download the configured model if it is missing, then load it.
    pub async fn prepare(cfg: &ServerConfig) -> Result<Self> {
        let info = EngineInfo::from_config(cfg);
        require_compiled(&cfg.engine.backend)?;
        ensure_model_downloaded(cfg).await?;

        let app_config = cfg.to_app_config();
        info!("Loading {} — this can take a while for a large model", info.model_id());
        let backend = tokio::task::spawn_blocking(move || -> Result<Box<dyn TranscriptionBackend>> {
            let mut backend = voxctrl_inference::build_backend(&app_config);
            backend.load()?;
            Ok(backend)
        })
        .await
        .context("The model loading task stopped unexpectedly")?
        .with_context(|| format!("Could not load {}", info.model_id()))?;

        let whisper_language = (cfg.engine.backend == BackendChoice::WhisperCpp)
            .then(|| normalize_language(&cfg.engine.whisper_cpp.language))
            .flatten();

        Ok(Self {
            gate: Gate::default(),
            max_waiting: MAX_WAITING_DICTATIONS,
            running_live: AtomicU64::new(0),
            preempted_live: AtomicU64::new(0),
            backend: Mutex::new(backend),
            info,
            whisper_language,
        })
    }

    /// Wrap a backend that is already loaded. For tests.
    #[cfg(test)]
    pub fn from_backend(backend: Box<dyn TranscriptionBackend>, info: EngineInfo) -> Self {
        Self {
            gate: Gate::default(),
            max_waiting: MAX_WAITING_DICTATIONS,
            running_live: AtomicU64::new(0),
            preempted_live: AtomicU64::new(0),
            backend: Mutex::new(backend),
            info,
            whisper_language: None,
        }
    }

    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// A number for a live pass about to be made, by which it can be stopped:
    /// `voxctrl_inference::cancel_interim_pass(ticket)` ends it between two
    /// steps of the model. The HTTP layer does that when the client that asked
    /// for the pass has hung up, so the server does not finish work nobody is
    /// waiting for while a dictation queues behind it.
    pub fn live_ticket(&self) -> u64 {
        LIVE_TICKETS.fetch_add(1, Ordering::SeqCst)
    }

    /// [`transcribe_as`](Self::transcribe_as) with a ticket of its own for a
    /// live pass. For tests, which have no client that could hang up.
    #[cfg(test)]
    pub fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
        prompt: Option<String>,
        purpose: Purpose,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let ticket = (purpose == Purpose::Live).then(|| self.live_ticket());
        self.transcribe_as(audio, language, prompt, purpose, ticket)
    }

    /// Transcribe 16 kHz mono audio. Blocking: call it off the async runtime.
    ///
    /// `language` is the request's own choice and wins over the configured one;
    /// `prompt` is the client's vocabulary hint. `live_ticket` is the number
    /// the caller took from [`live_ticket`](Self::live_ticket) to be able to
    /// stop a live pass; it is ignored for a dictation, which nothing may stop.
    pub fn transcribe_as(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
        prompt: Option<String>,
        purpose: Purpose,
        live_ticket: Option<u64>,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let live_ticket = live_ticket.filter(|_| purpose == Purpose::Live);
        let result = self.run(audio, language, prompt, purpose, live_ticket);
        // The pass is over, so nothing will ask again whether it was stopped.
        if let Some(ticket) = live_ticket {
            voxctrl_inference::forget_interim_pass(ticket);
        }
        result
    }

    fn run(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
        prompt: Option<String>,
        purpose: Purpose,
        live_ticket: Option<u64>,
    ) -> Result<TranscriptionResult, TranscribeError> {
        let duration_ms = (audio.len() / 16) as u32;
        let language = language.as_deref().and_then(normalize_language).or_else(|| self.whisper_language.clone());

        if crate::audio::rms(&audio) < SILENCE_RMS {
            return Ok(TranscriptionResult {
                text: String::new(),
                language: language.unwrap_or_else(|| "en".into()),
                language_probability: 0.0,
                duration_ms,
                inference_ms: 0,
                word_timestamps: None,
            });
        }

        let request = TranscribeRequest {
            audio,
            language,
            word_timestamps: false,
            initial_prompt: prompt.filter(|p| !p.trim().is_empty()),
            // What lets a live pass be stopped part-way. `None` for a dictation.
            interim_session: live_ticket,
        };
        if purpose == Purpose::Dictation {
            // Someone has finished speaking. A live pass that has the model
            // only feeds an overlay, and its result is about to be replaced
            // (often by this very dictation): stop it now, so this request
            // waits for one step of the model and not for the whole pass.
            let running = self.running_live.load(Ordering::SeqCst);
            if running != 0 {
                self.preempted_live.fetch_max(running, Ordering::SeqCst);
                voxctrl_inference::cancel_interim_pass(running);
            }
        }
        let _turn = self.gate.enter(purpose, self.max_waiting)?;
        // A panic in an earlier request must not take the server down for
        // every later one, so a poisoned lock is recovered rather than unwrapped.
        let backend = self.backend.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        self.running_live.store(live_ticket.unwrap_or(0), Ordering::SeqCst);
        let outcome = backend.transcribe(&request);
        self.running_live.store(0, Ordering::SeqCst);
        let mut result = outcome.map_err(|e| match live_ticket {
            // Stopped to let a dictation through: routine, and to the client
            // the same as having found the model busy.
            Some(ticket) if ticket <= self.preempted_live.load(Ordering::SeqCst) => {
                TranscribeError::Busy(Purpose::Live)
            }
            _ => TranscribeError::Failed(e),
        })?;
        // The server otherwise serves the model's words untouched, and leaves
        // post-processing to the client. A decoder stuck on one word is the
        // exception: it is the model's fault, not a matter of the client's
        // taste, and a client that is not the VoxCtrl app has no guard for it.
        if let std::borrow::Cow::Owned(fixed) =
            voxctrl_inference::finalize::without_repetition_loops(&result.text, "transcript")
        {
            result.text = fixed;
        }
        Ok(result)
    }

    /// A smaller queue, for tests that fill it.
    #[cfg(test)]
    pub fn with_max_waiting(mut self, max_waiting: usize) -> Self {
        self.max_waiting = max_waiting;
        self
    }
}

/// A model given as a path to a file is named by the file, without its
/// directory or extension, so a server does not publish its filesystem layout
/// in `/v1/models`. A plain model name is returned as is.
fn model_name(model: &str) -> String {
    let path = std::path::Path::new(model);
    if path.is_absolute() || model.ends_with(".bin") {
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            return stem.to_string();
        }
    }
    model.to_string()
}

/// `auto` and blank mean "let the model detect it".
fn normalize_language(language: &str) -> Option<String> {
    let trimmed = language.trim();
    (!trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("auto")).then(|| trimmed.to_lowercase())
}

/// The inference crate quietly falls back to whisper.cpp when a backend was
/// not compiled in. On a desktop that keeps dictation working; on a server it
/// would mean running a different model than the one configured, so refuse.
fn require_compiled(backend: &BackendChoice) -> Result<()> {
    match backend {
        BackendChoice::Moonshine if !voxctrl_inference::MOONSHINE_COMPILED => bail!(
            "engine.backend is 'moonshine', but this server was built without it. \
             Rebuild with `--features moonshine`, or choose another backend."
        ),
        BackendChoice::Parakeet if !voxctrl_inference::PARAKEET_COMPILED => bail!(
            "engine.backend is 'parakeet', but this server was built without it. \
             Rebuild with `--features parakeet`, or choose another backend."
        ),
        _ => Ok(()),
    }
}

async fn ensure_model_downloaded(cfg: &ServerConfig) -> Result<()> {
    let e = &cfg.engine;
    match e.backend {
        BackendChoice::WhisperCpp => {
            let size = &e.whisper_cpp.model_size;
            let dir = &e.whisper_cpp.model_dir;
            // A path to a model file is used as given; there is nothing to fetch.
            let is_path = size.ends_with(".bin") || std::path::Path::new(size).is_absolute();
            if !is_path && !voxctrl_inference::whisper_cpp::is_model_downloaded(size, dir) {
                info!("Downloading the Whisper '{size}' model (first start only)");
                voxctrl_inference::whisper_cpp::download_model(size, dir)
                    .await
                    .with_context(|| format!("Could not download the Whisper '{size}' model"))?;
            }
        }
        BackendChoice::Moonshine => {
            #[cfg(feature = "moonshine")]
            {
                let size = &e.moonshine.model_size;
                if !voxctrl_inference::moonshine::is_model_downloaded(size, "") {
                    info!("Downloading the Moonshine '{size}' model (first start only)");
                    voxctrl_inference::moonshine::download_model(size, "")
                        .await
                        .with_context(|| format!("Could not download the Moonshine '{size}' model"))?;
                }
            }
        }
        BackendChoice::Parakeet => {
            #[cfg(feature = "parakeet")]
            {
                let size = &e.parakeet.model_size;
                if !voxctrl_inference::parakeet::is_model_downloaded(size, "") {
                    info!("Downloading the Parakeet '{size}' model (first start only)");
                    voxctrl_inference::parakeet::download_model(size, "")
                        .await
                        .with_context(|| format!("Could not download the Parakeet '{size}' model"))?;
                }
            }
        }
        BackendChoice::RemoteOpenAi => {}
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex};

    /// What a [`FakeBackend`] was last asked to transcribe.
    #[derive(Debug, Default, Clone)]
    pub struct Seen {
        pub samples: usize,
        pub language: Option<String>,
        pub prompt: Option<String>,
        pub calls: usize,
    }

    /// A backend that returns fixed text and records what it was given.
    pub struct FakeBackend {
        pub text: String,
        pub fail: bool,
        pub seen: Arc<StdMutex<Seen>>,
    }

    impl TranscriptionBackend for FakeBackend {
        fn name(&self) -> &str {
            "fake"
        }
        fn load(&mut self) -> Result<()> {
            Ok(())
        }
        fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
            let mut seen = self.seen.lock().unwrap();
            seen.samples = req.audio.len();
            seen.language = req.language.clone();
            seen.prompt = req.initial_prompt.clone();
            seen.calls += 1;
            if self.fail {
                bail!("the model fell over");
            }
            Ok(TranscriptionResult {
                text: self.text.clone(),
                language: req.language.clone().unwrap_or_else(|| "en".into()),
                language_probability: 1.0,
                duration_ms: (req.audio.len() / 16) as u32,
                inference_ms: 7,
                word_timestamps: None,
            })
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
    }

    pub fn fake_engine(text: &str, fail: bool) -> (Engine, Arc<StdMutex<Seen>>) {
        let seen = Arc::new(StdMutex::new(Seen::default()));
        let backend = FakeBackend { text: text.into(), fail, seen: seen.clone() };
        let info = EngineInfo { backend: "fake".into(), model: "test".into(), language: "auto".into() };
        (Engine::from_backend(Box::new(backend), info), seen)
    }

    /// A backend whose live passes run until they are told to stop, as a real
    /// one does between steps of its model. Dictations return at once.
    pub struct StoppableBackend {
        /// Told when a live pass has the model...
        pub started: std::sync::mpsc::Sender<()>,
        /// ...and when it has been stopped.
        pub stopped: std::sync::mpsc::Sender<()>,
    }

    impl TranscriptionBackend for StoppableBackend {
        fn name(&self) -> &str {
            "stoppable"
        }
        fn load(&mut self) -> Result<()> {
            Ok(())
        }
        fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
            if let Some(ticket) = req.interim_session {
                let _ = self.started.send(());
                let waited = std::time::Instant::now();
                while !voxctrl_inference::interim_cancelled(ticket) {
                    assert!(waited.elapsed() < std::time::Duration::from_secs(10), "the live pass was never stopped");
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                let _ = self.stopped.send(());
                bail!("live pass cancelled");
            }
            Ok(TranscriptionResult {
                text: "the dictation".into(),
                language: "en".into(),
                language_probability: 1.0,
                duration_ms: 0,
                inference_ms: 0,
                word_timestamps: None,
            })
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
    }

    /// An engine on a [`StoppableBackend`], with its "started" and "stopped"
    /// signals.
    pub fn stoppable_engine() -> (Engine, std::sync::mpsc::Receiver<()>, std::sync::mpsc::Receiver<()>) {
        let (started_tx, started) = std::sync::mpsc::channel();
        let (stopped_tx, stopped) = std::sync::mpsc::channel();
        let backend = StoppableBackend { started: started_tx, stopped: stopped_tx };
        let info = EngineInfo { backend: "stoppable".into(), model: "test".into(), language: "auto".into() };
        (Engine::from_backend(Box::new(backend), info), started, stopped)
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::fake_engine;
    use super::*;

    #[test]
    fn silence_is_answered_without_running_the_model() {
        let (engine, seen) = fake_engine("should not appear", false);
        let out = engine.transcribe(vec![0.0; 3200], None, None, Purpose::Dictation).unwrap();
        assert_eq!(out.text, "");
        assert_eq!(out.duration_ms, 200);
        assert_eq!(seen.lock().unwrap().calls, 0);
    }

    #[test]
    fn empty_audio_is_answered_without_running_the_model() {
        let (engine, seen) = fake_engine("should not appear", false);
        assert_eq!(engine.transcribe(Vec::new(), None, None, Purpose::Dictation).unwrap().text, "");
        assert_eq!(seen.lock().unwrap().calls, 0);
    }

    #[test]
    fn speech_reaches_the_model_with_its_language_and_prompt() {
        let (engine, seen) = fake_engine("hello there", false);
        let out = engine
            .transcribe(vec![0.2; 16_000], Some(" EN ".into()), Some("VoxCtrl, Parakeet".into()), Purpose::Dictation)
            .unwrap();
        assert_eq!(out.text, "hello there");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.calls, 1);
        assert_eq!(seen.samples, 16_000);
        assert_eq!(seen.language.as_deref(), Some("en"));
        assert_eq!(seen.prompt.as_deref(), Some("VoxCtrl, Parakeet"));
    }

    #[test]
    fn a_looped_transcript_is_served_once() {
        let looped = format!("call me {}back", "right ".repeat(50));
        let (engine, _) = fake_engine(&looped, false);
        let out = engine.transcribe(vec![0.2; 16_000], None, None, Purpose::Dictation).unwrap();
        assert_eq!(out.text, "call me right back");
    }

    #[test]
    fn auto_language_and_blank_prompt_are_passed_as_none() {
        let (engine, seen) = fake_engine("x", false);
        engine.transcribe(vec![0.2; 1600], Some("auto".into()), Some("   ".into()), Purpose::Dictation).unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.language, None);
        assert_eq!(seen.prompt, None);
    }

    #[test]
    fn a_model_error_is_returned_not_swallowed() {
        let (engine, _) = fake_engine("x", true);
        let err = engine.transcribe(vec![0.2; 1600], None, None, Purpose::Dictation).unwrap_err();
        assert!(err.to_string().contains("fell over"));
    }

    #[test]
    fn model_id_names_backend_and_model() {
        let mut cfg = ServerConfig::default();
        cfg.engine.backend = BackendChoice::Parakeet;
        cfg.engine.parakeet.model_size = "tdt-0.6b-v3".into();
        assert_eq!(EngineInfo::from_config(&cfg).model_id(), "parakeet/tdt-0.6b-v3");
    }

    #[test]
    fn a_model_given_as_a_path_is_named_by_its_file() {
        assert_eq!(model_name("large-v3-turbo"), "large-v3-turbo");
        assert_eq!(model_name("base.en"), "base.en");
        assert_eq!(model_name("/srv/models/ggml-custom-q5.bin"), "ggml-custom-q5");
        assert_eq!(model_name("my-finetune.bin"), "my-finetune");

        let mut cfg = ServerConfig::default();
        cfg.engine.whisper_cpp.model_size = "/srv/models/ggml-custom-q5.bin".into();
        assert_eq!(EngineInfo::from_config(&cfg).model_id(), "whisper-cpp/ggml-custom-q5");
    }

    /// A backend that holds the model until told to let go, so a test can
    /// see what other requests do meanwhile.
    struct HeldBackend {
        started: std::sync::mpsc::Sender<()>,
        release: StdMutex<std::sync::mpsc::Receiver<()>>,
    }

    impl TranscriptionBackend for HeldBackend {
        fn name(&self) -> &str {
            "held"
        }
        fn load(&mut self) -> Result<()> {
            Ok(())
        }
        fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
            self.started.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            Ok(TranscriptionResult {
                text: format!("{} samples", req.audio.len()),
                language: "en".into(),
                language_probability: 1.0,
                duration_ms: 0,
                inference_ms: 0,
                word_timestamps: None,
            })
        }
        fn unload(&mut self) {}
        fn is_loaded(&self) -> bool {
            true
        }
    }

    use std::sync::Mutex as StdMutex;

    fn held_engine(max_waiting: usize) -> (Arc<Engine>, std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (started_tx, started) = std::sync::mpsc::channel();
        let (release, release_rx) = std::sync::mpsc::channel();
        let backend = HeldBackend { started: started_tx, release: StdMutex::new(release_rx) };
        let info = EngineInfo { backend: "held".into(), model: "test".into(), language: "auto".into() };
        (Arc::new(Engine::from_backend(Box::new(backend), info).with_max_waiting(max_waiting)), started, release)
    }

    use std::sync::Arc;

    fn speech(samples: usize) -> Vec<f32> {
        vec![0.2; samples]
    }

    #[test]
    fn a_live_pass_never_waits_for_the_model() {
        let (engine, started, release) = held_engine(MAX_WAITING_DICTATIONS);
        let first = {
            let engine = engine.clone();
            std::thread::spawn(move || engine.transcribe(speech(100), None, None, Purpose::Dictation))
        };
        started.recv().unwrap();

        // The model is busy with a dictation: a live pass is turned away at once.
        let live = engine.transcribe(speech(200), None, None, Purpose::Live);
        assert!(matches!(live, Err(TranscribeError::Busy(Purpose::Live))), "{live:?}");

        release.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap().text, "100 samples");

        // Free again: a live pass runs.
        let live = {
            let engine = engine.clone();
            std::thread::spawn(move || engine.transcribe(speech(300), None, None, Purpose::Live))
        };
        started.recv().unwrap();
        release.send(()).unwrap();
        assert_eq!(live.join().unwrap().unwrap().text, "300 samples");
    }

    #[test]
    fn dictations_queue_up_to_the_limit_and_live_passes_yield_to_them() {
        let (engine, started, release) = held_engine(2);
        let spawn = |samples: usize| {
            let engine = engine.clone();
            std::thread::spawn(move || engine.transcribe(speech(samples), None, None, Purpose::Dictation))
        };
        let running = spawn(1);
        started.recv().unwrap();
        let waiting: Vec<_> = (2..=3).map(spawn).collect();
        // Let both reach the queue.
        while engine.gate.state.lock().unwrap().waiting < 2 {
            std::thread::yield_now();
        }

        // The queue is full: one more dictation is refused, not queued.
        let refused = engine.transcribe(speech(4), None, None, Purpose::Dictation);
        assert!(matches!(refused, Err(TranscribeError::Busy(Purpose::Dictation))), "{refused:?}");
        // A live pass may not jump the queue.
        let live = engine.transcribe(speech(5), None, None, Purpose::Live);
        assert!(matches!(live, Err(TranscribeError::Busy(Purpose::Live))), "{live:?}");

        // Each queued dictation runs in turn.
        release.send(()).unwrap();
        assert_eq!(running.join().unwrap().unwrap().text, "1 samples");
        for _ in 0..2 {
            started.recv().unwrap();
            release.send(()).unwrap();
        }
        let mut done: Vec<String> = waiting.into_iter().map(|w| w.join().unwrap().unwrap().text).collect();
        done.sort();
        assert_eq!(done, ["2 samples", "3 samples"]);
        let state = engine.gate.state.lock().unwrap();
        assert!(!state.busy && state.waiting == 0);
    }

    #[test]
    fn a_dictation_stops_the_live_pass_that_has_the_model() {
        let (engine, started, _stopped) = super::test_support::stoppable_engine();
        let engine = Arc::new(engine);
        let live = {
            let engine = engine.clone();
            std::thread::spawn(move || engine.transcribe(speech(1600), None, None, Purpose::Live))
        };
        started.recv().unwrap();

        // The live pass would run for ever; the dictation must not wait for it.
        let out = engine.transcribe(speech(1600), None, None, Purpose::Dictation).unwrap();
        assert_eq!(out.text, "the dictation");

        // To its client the stopped pass looks like any skipped one.
        let live = live.join().unwrap();
        assert!(matches!(live, Err(TranscribeError::Busy(Purpose::Live))), "{live:?}");
        let state = engine.gate.state.lock().unwrap();
        assert!(!state.busy && state.waiting == 0);
    }

    #[test]
    fn a_live_pass_stops_when_its_ticket_is_cancelled() {
        let (engine, started, _stopped) = super::test_support::stoppable_engine();
        let engine = Arc::new(engine);
        let ticket = engine.live_ticket();
        let live = {
            let engine = engine.clone();
            std::thread::spawn(move || engine.transcribe_as(speech(1600), None, None, Purpose::Live, Some(ticket)))
        };
        started.recv().unwrap();
        // What the HTTP layer does when the client has hung up.
        voxctrl_inference::cancel_interim_pass(ticket);
        // Nobody preempted it, so this is the model's own error, not "busy".
        let live = live.join().unwrap();
        assert!(matches!(live, Err(TranscribeError::Failed(_))), "{live:?}");
        // The model is free for the next request, and the ticket is spent.
        assert_eq!(engine.transcribe(speech(1600), None, None, Purpose::Dictation).unwrap().text, "the dictation");
        assert!(!voxctrl_inference::interim_cancelled(ticket));
    }

    #[test]
    fn a_failed_transcription_frees_the_model() {
        let (engine, _) = fake_engine("x", true);
        assert!(matches!(
            engine.transcribe(speech(1600), None, None, Purpose::Dictation),
            Err(TranscribeError::Failed(_))
        ));
        let live = engine.transcribe(speech(1600), None, None, Purpose::Live);
        assert!(matches!(live, Err(TranscribeError::Failed(_))), "the model is free again: {live:?}");
    }

    #[test]
    fn normalize_language_treats_auto_and_blank_as_unset() {
        assert_eq!(normalize_language("auto"), None);
        assert_eq!(normalize_language("AUTO"), None);
        assert_eq!(normalize_language("  "), None);
        assert_eq!(normalize_language("De"), Some("de".into()));
    }
}
