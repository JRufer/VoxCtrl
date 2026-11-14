//! The one speech model this server runs: fetching it, loading it, and
//! transcribing with it one request at a time.

use std::sync::Mutex;

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

pub struct Engine {
    /// One request at a time. The backends hold a single inference state, and
    /// two transcriptions sharing one GPU would only take turns anyway.
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

        Ok(Self { backend: Mutex::new(backend), info, whisper_language })
    }

    /// Wrap a backend that is already loaded. For tests.
    #[cfg(test)]
    pub fn from_backend(backend: Box<dyn TranscriptionBackend>, info: EngineInfo) -> Self {
        Self { backend: Mutex::new(backend), info, whisper_language: None }
    }

    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    /// Transcribe 16 kHz mono audio. Blocking: call it off the async runtime.
    ///
    /// `language` is the request's own choice and wins over the configured one;
    /// `prompt` is the client's vocabulary hint.
    pub fn transcribe(
        &self,
        audio: Vec<f32>,
        language: Option<String>,
        prompt: Option<String>,
    ) -> Result<TranscriptionResult> {
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
            interim_session: None,
        };
        // A panic in an earlier request must not take the server down for
        // every later one, so a poisoned lock is recovered rather than unwrapped.
        let backend = self.backend.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        backend.transcribe(&request)
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
}

#[cfg(test)]
mod tests {
    use super::test_support::fake_engine;
    use super::*;

    #[test]
    fn silence_is_answered_without_running_the_model() {
        let (engine, seen) = fake_engine("should not appear", false);
        let out = engine.transcribe(vec![0.0; 3200], None, None).unwrap();
        assert_eq!(out.text, "");
        assert_eq!(out.duration_ms, 200);
        assert_eq!(seen.lock().unwrap().calls, 0);
    }

    #[test]
    fn empty_audio_is_answered_without_running_the_model() {
        let (engine, seen) = fake_engine("should not appear", false);
        assert_eq!(engine.transcribe(Vec::new(), None, None).unwrap().text, "");
        assert_eq!(seen.lock().unwrap().calls, 0);
    }

    #[test]
    fn speech_reaches_the_model_with_its_language_and_prompt() {
        let (engine, seen) = fake_engine("hello there", false);
        let out = engine
            .transcribe(vec![0.2; 16_000], Some(" EN ".into()), Some("VoxCtrl, Parakeet".into()))
            .unwrap();
        assert_eq!(out.text, "hello there");
        let seen = seen.lock().unwrap();
        assert_eq!(seen.calls, 1);
        assert_eq!(seen.samples, 16_000);
        assert_eq!(seen.language.as_deref(), Some("en"));
        assert_eq!(seen.prompt.as_deref(), Some("VoxCtrl, Parakeet"));
    }

    #[test]
    fn auto_language_and_blank_prompt_are_passed_as_none() {
        let (engine, seen) = fake_engine("x", false);
        engine.transcribe(vec![0.2; 1600], Some("auto".into()), Some("   ".into())).unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.language, None);
        assert_eq!(seen.prompt, None);
    }

    #[test]
    fn a_model_error_is_returned_not_swallowed() {
        let (engine, _) = fake_engine("x", true);
        let err = engine.transcribe(vec![0.2; 1600], None, None).unwrap_err();
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

    #[test]
    fn normalize_language_treats_auto_and_blank_as_unset() {
        assert_eq!(normalize_language("auto"), None);
        assert_eq!(normalize_language("AUTO"), None);
        assert_eq!(normalize_language("  "), None);
        assert_eq!(normalize_language("De"), Some("de".into()));
    }
}
