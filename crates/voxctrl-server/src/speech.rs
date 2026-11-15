//! Text-to-speech on the server, for clients whose TTS engine is "VoxCtrl
//! server".
//!
//! The server owner decides which engines exist here (`[speech] engines`, and
//! each engine's settings under `[tts]`). A client picks among them, and
//! picks a voice: one of the engine's own, or one of the clips that client
//! has uploaded. Nothing a client sends makes the server download anything.
//!
//! The server synthesizes and returns a WAV file; the client plays it. A
//! client's own speed setting and stop key still apply, since playback is
//! theirs.

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use tracing::info;
use voxctrl_config::{TtsConfig, TtsEngine};
use voxctrl_tts::synth::{self, Synthesizer};

/// What a client asked to have spoken.
#[derive(Debug, Clone, Default)]
pub struct SpeechRequest {
    pub text: String,
    /// An engine id from [`Speaker::engines`]. `None`, or an id this server
    /// does not offer, means the server's default engine: OpenAI clients send
    /// model names of their own here.
    pub engine: Option<String>,
    /// A voice of that engine, or `custom:<name>` for one this client
    /// uploaded. `None` means the engine's configured voice.
    pub voice: Option<String>,
    /// The folder holding this client's uploaded voices, when it sent an id.
    pub client_voices: Option<PathBuf>,
}

#[derive(Debug)]
pub enum SpeakError {
    /// The request asks for something this server does not have. The text is
    /// for the client.
    Invalid(String),
    /// The engine failed. Details stay in the server log.
    Engine(anyhow::Error),
}

/// One engine this server offers, for a client's pickers.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct EngineOffer {
    /// As config files spell it, e.g. `kitten_tts_2`.
    pub id: String,
    pub name: String,
    /// The engine's own voices that can be asked for by name.
    pub voices: Vec<String>,
    /// The voice used when a request names none.
    pub default_voice: Option<String>,
    /// Whether the engine can speak with a clip the client uploads.
    pub custom_voices: bool,
}

/// Something that can turn text into a WAV file. A trait so the HTTP layer
/// can be tested without an engine installed.
pub trait Speaker: Send + Sync {
    /// Blocking, and can take seconds: call it off the async runtime.
    fn speak(&self, request: &SpeechRequest) -> Result<Vec<u8>, SpeakError>;
    /// The engines on offer, the default first.
    fn engines(&self) -> Vec<EngineOffer>;
}

/// The app's own TTS engines, one model loaded at a time.
pub struct EngineSpeaker {
    config: TtsConfig,
    offered: Vec<TtsEngine>,
    worker: SpeechWorker,
}

impl EngineSpeaker {
    /// Download what each offered engine needs, and load the default one, so
    /// the first request pays for neither. `engines` is what to offer besides
    /// `config.engine`, which is always offered and is the default.
    pub async fn prepare(config: TtsConfig, engines: &[TtsEngine]) -> Result<Self> {
        let mut offered = vec![config.engine.clone()];
        for engine in engines {
            if !offered.contains(engine) {
                offered.push(engine.clone());
            }
        }
        for engine in &offered {
            if !synth::supports(engine) {
                anyhow::bail!(
                    "The TTS engine \"{}\" cannot run on the server. Use piper, espeak, pocket_tts, breeze_tts_2, \
                     vox_cpm_2 or kitten_tts_2.",
                    synth::engine_id(engine)
                );
            }
        }
        for engine in &offered {
            info!("Preparing text-to-speech ({})", synth::engine_id(engine));
            let for_engine = TtsConfig { engine: engine.clone(), ..config.clone() };
            synth::ensure_assets(&for_engine).await.with_context(|| {
                format!(
                    "Could not get the {} text-to-speech engine ready. Check this machine's internet access, \
                     or take it out of the engines this server offers.",
                    synth::engine_id(engine)
                )
            })?;
            if *engine == TtsEngine::PocketTts {
                fetch_pocket_voices(&for_engine).await;
            }
        }
        let worker_config = config.clone();
        let worker = tokio::task::spawn_blocking(move || {
            SpeechWorker::start(move || {
                let mut synthesizer = Synthesizer::new();
                synthesizer.load(&worker_config)?;
                Ok(move |job: &PlannedSpeech| synthesizer.synthesize(&job.config, &job.text, None))
            })
        })
        .await
        .context("Loading the text-to-speech model stopped unexpectedly")?
        .context("Could not load the text-to-speech model")?;
        Ok(Self { config, offered, worker })
    }
}

/// Pocket-TTS's built-in voices are a small file each, and only the
/// configured one comes with the model. Offering the engine means offering
/// its voices, so fetch the rest. One that cannot be fetched is logged and
/// left out of what clients are offered; the engine still works.
async fn fetch_pocket_voices(config: &TtsConfig) {
    let token = voxctrl_tts::effective_hf_token(config.hf_token.as_deref());
    for voice in voxctrl_tts::POCKET_TTS_VOICES {
        if voxctrl_tts::is_pocket_tts_ready(voice.id, &config.pocket_tts.voice_dir) {
            continue;
        }
        if let Err(e) =
            voxctrl_tts::download_pocket_tts_assets(voice.id, &config.pocket_tts.voice_dir, token.clone()).await
        {
            tracing::warn!("Could not download the Pocket-TTS voice '{}'; it will not be offered: {e:#}", voice.id);
        }
    }
}

/// A request turned into the settings to run it with.
struct PlannedSpeech {
    config: TtsConfig,
    text: String,
}

fn engine_name(engine: &TtsEngine) -> &'static str {
    match engine {
        TtsEngine::Piper => "Piper",
        TtsEngine::Espeak => "eSpeak NG",
        TtsEngine::PocketTts => "Pocket-TTS",
        TtsEngine::BreezeTts2 => "Breeze-TTS-2",
        TtsEngine::VoxCpm2 => "VoxCPM2",
        TtsEngine::KittenTts2 => "Kitten TTS 2",
        TtsEngine::InflectMicro => "Inflect-Micro",
        TtsEngine::Server => "VoxCtrl server",
    }
}

/// Engines that can speak with a reference clip.
fn clones(engine: &TtsEngine) -> bool {
    matches!(engine, TtsEngine::PocketTts | TtsEngine::BreezeTts2 | TtsEngine::VoxCpm2 | TtsEngine::KittenTts2)
}

/// The engine's own voices that are ready to use on this machine.
fn builtin_voices(config: &TtsConfig, engine: &TtsEngine) -> Vec<String> {
    match engine {
        // Only voices already downloaded here: a client cannot make the
        // server fetch one.
        TtsEngine::Piper => voxctrl_tts::PIPER_VOICES
            .iter()
            .filter(|v| voxctrl_tts::is_voice_downloaded(v.name, &config.voice_dir))
            .map(|v| v.name.to_string())
            .collect(),
        // Each built-in voice is a file of its own; one that is not on disk
        // would fail when asked for, so it is not offered.
        TtsEngine::PocketTts => voxctrl_tts::pocket_tts_voice_catalogue(&config.pocket_tts.voice_dir)
            .into_iter()
            .map(|v| v.id)
            .filter(|id| voxctrl_tts::is_pocket_tts_ready(id, &config.pocket_tts.voice_dir))
            .collect(),
        TtsEngine::KittenTts2 => voxctrl_tts::KITTEN_TTS_2_VOICES.iter().map(|v| v.to_string()).collect(),
        _ => Vec::new(),
    }
}

fn offer(config: &TtsConfig, engine: &TtsEngine) -> EngineOffer {
    let for_engine = TtsConfig { engine: engine.clone(), ..config.clone() };
    EngineOffer {
        id: synth::engine_id(engine),
        name: engine_name(engine).to_string(),
        voices: builtin_voices(config, engine),
        default_voice: configured_voice(&for_engine),
        custom_voices: clones(engine),
    }
}

/// Work out the settings for one request, or say what is wrong with it.
/// Pure apart from checking that an uploaded clip exists.
fn plan(base: &TtsConfig, offered: &[TtsEngine], request: &SpeechRequest) -> Result<TtsConfig, String> {
    let engine = request
        .engine
        .as_deref()
        .and_then(|id| offered.iter().find(|e| synth::engine_id(e) == id.trim()))
        .unwrap_or(&base.engine)
        .clone();
    let mut config = TtsConfig { engine: engine.clone(), ..base.clone() };
    let Some(voice) = request.voice.as_deref().map(str::trim).filter(|v| !v.is_empty()) else {
        return Ok(config);
    };
    let name = engine_name(&engine);

    if let Some(custom) = voice.strip_prefix("custom:") {
        if !clones(&engine) {
            return Err(format!("{name} cannot speak with an uploaded voice. Choose one of its own voices."));
        }
        let folder = request
            .client_voices
            .as_ref()
            .filter(|dir| crate::voices::valid_voice_name(custom) && dir.join(format!("{custom}.wav")).is_file())
            .ok_or_else(|| format!("The voice \"{custom}\" has not been uploaded to this server by this client."))?;
        let folder = folder.to_string_lossy().into_owned();
        match engine {
            TtsEngine::PocketTts => {
                config.pocket_tts.voice = custom.to_string();
                config.pocket_tts.voice_dir = folder;
            }
            TtsEngine::BreezeTts2 => {
                config.breeze_tts_2.voice_mode = "clone".into();
                config.breeze_tts_2.cloned_voice = custom.to_string();
                config.breeze_tts_2.voice_dir = folder;
            }
            TtsEngine::VoxCpm2 => {
                config.vox_cpm_2.voice_mode = "clone".into();
                config.vox_cpm_2.cloned_voice = custom.to_string();
                config.vox_cpm_2.voice_dir = folder;
            }
            TtsEngine::KittenTts2 => {
                config.kitten_tts_2.voice_mode = "clone".into();
                config.kitten_tts_2.cloned_voice = custom.to_string();
                config.kitten_tts_2.voice_dir = folder;
            }
            _ => unreachable!("clones() covers exactly these engines"),
        }
        return Ok(config);
    }

    // Only names this server lists: the name becomes part of a file path.
    if !builtin_voices(base, &engine).iter().any(|v| v == voice) {
        // A generic OpenAI client names voices this server has never heard
        // of ("alloy"); it gets the engine's own voice rather than an error,
        // unless it asked for this engine by name and so should know better.
        let asked_for_engine = request.engine.as_deref().map(str::trim) == Some(synth::engine_id(&engine).as_str());
        return if asked_for_engine {
            Err(format!("{name} on this server has no voice called \"{voice}\"."))
        } else {
            Ok(config)
        };
    }
    match engine {
        TtsEngine::Piper => config.voice = voice.to_string(),
        TtsEngine::PocketTts => config.pocket_tts.voice = voice.to_string(),
        TtsEngine::KittenTts2 => {
            config.kitten_tts_2.voice_mode = "preset".into();
            config.kitten_tts_2.voice = voice.to_string();
        }
        _ => {}
    }
    Ok(config)
}

struct Job {
    speech: PlannedSpeech,
    reply: mpsc::Sender<Result<Vec<u8>>>,
}

/// One thread that owns the engine for as long as the server runs.
///
/// The engine must not be driven from whichever pool thread a request lands
/// on. The neural engines keep a helper process (audio.cpp's server) that is
/// told to die with its parent, and on Linux "parent" means the *thread* that
/// started it. Pool threads are retired after a few idle seconds, which took
/// the helper with them and left every later request with nothing to connect
/// to. Started and used on this one thread, the helper lives as long as it.
struct SpeechWorker {
    // One request at a time: the engines hold one model and one helper.
    jobs: Mutex<mpsc::Sender<Job>>,
}

impl SpeechWorker {
    /// Start the thread, run `load` on it, and return once `load` has. `load`
    /// returns the function that speaks, which stays on that thread.
    fn start<L, S>(load: L) -> Result<Self>
    where
        L: FnOnce() -> Result<S> + Send + 'static,
        S: FnMut(&PlannedSpeech) -> Result<Vec<u8>>,
    {
        let (jobs, inbox) = mpsc::channel::<Job>();
        let (loaded_tx, loaded) = mpsc::channel::<Result<()>>();
        std::thread::Builder::new()
            .name("voxctrl-speech".into())
            .spawn(move || {
                let mut speak = match load() {
                    Ok(speak) => {
                        let _ = loaded_tx.send(Ok(()));
                        speak
                    }
                    Err(e) => {
                        let _ = loaded_tx.send(Err(e));
                        return;
                    }
                };
                // Ends when the server drops the last sender, at shutdown.
                for job in inbox {
                    let _ = job.reply.send(speak(&job.speech));
                }
            })
            .context("Could not start the speech thread")?;
        loaded.recv().map_err(|_| anyhow!("The speech thread stopped while loading the model"))??;
        Ok(Self { jobs: Mutex::new(jobs) })
    }

    /// Blocking: waits its turn, then for the audio.
    fn speak(&self, speech: PlannedSpeech) -> Result<Vec<u8>> {
        let (reply, answer) = mpsc::channel();
        // Held to the end, so requests are answered in the order they came.
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.send(Job { speech, reply })
            .map_err(|_| anyhow!("The speech engine has stopped. Restart the server."))?;
        answer.recv().map_err(|_| anyhow!("The speech engine stopped while speaking. Restart the server."))?
    }
}

/// The voice the configured engine speaks with, for engines that name one.
pub fn configured_voice(config: &TtsConfig) -> Option<String> {
    let named = |voice: &str| Some(voice.to_string()).filter(|v| !v.trim().is_empty());
    match config.engine {
        TtsEngine::Piper => named(&config.voice),
        TtsEngine::PocketTts => named(&config.pocket_tts.voice),
        TtsEngine::KittenTts2 => named(&config.kitten_tts_2.voice),
        _ => None,
    }
}

impl Speaker for EngineSpeaker {
    fn speak(&self, request: &SpeechRequest) -> Result<Vec<u8>, SpeakError> {
        let config = plan(&self.config, &self.offered, request).map_err(SpeakError::Invalid)?;
        self.worker.speak(PlannedSpeech { config, text: request.text.clone() }).map_err(SpeakError::Engine)
    }

    fn engines(&self) -> Vec<EngineOffer> {
        self.offered.iter().map(|engine| offer(&self.config, engine)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn say(text: &str) -> PlannedSpeech {
        PlannedSpeech { config: TtsConfig::default(), text: text.to_string() }
    }

    fn request(engine: Option<&str>, voice: Option<&str>, client_voices: Option<&std::path::Path>) -> SpeechRequest {
        SpeechRequest {
            text: "hi".into(),
            engine: engine.map(str::to_string),
            voice: voice.map(str::to_string),
            client_voices: client_voices.map(|p| p.to_path_buf()),
        }
    }

    fn base() -> (TtsConfig, Vec<TtsEngine>) {
        let config = TtsConfig { engine: TtsEngine::KittenTts2, ..Default::default() };
        (config, vec![TtsEngine::KittenTts2, TtsEngine::PocketTts, TtsEngine::Espeak])
    }

    #[test]
    fn a_request_that_names_nothing_gets_the_servers_default() {
        let (config, offered) = base();
        let planned = plan(&config, &offered, &request(None, None, None)).unwrap();
        assert_eq!(planned, config);
    }

    #[test]
    fn a_client_picks_among_the_engines_on_offer_and_no_others() {
        let (config, offered) = base();
        let planned = plan(&config, &offered, &request(Some("pocket_tts"), None, None)).unwrap();
        assert_eq!(planned.engine, TtsEngine::PocketTts);
        // Not offered here, and what an OpenAI client sends: both fall back
        // to the default rather than starting an engine nobody set up.
        for other in ["piper", "tts-1", ""] {
            let planned = plan(&config, &offered, &request(Some(other), None, None)).unwrap();
            assert_eq!(planned.engine, TtsEngine::KittenTts2, "{other}");
        }
    }

    #[test]
    fn a_built_in_voice_is_set_on_the_engine_asked_for() {
        let (config, offered) = base();
        let planned = plan(&config, &offered, &request(Some("kitten_tts_2"), Some("Luna"), None)).unwrap();
        assert_eq!(planned.kitten_tts_2.voice, "Luna");
        assert_eq!(planned.kitten_tts_2.voice_mode, "preset");

        let err = plan(&config, &offered, &request(Some("kitten_tts_2"), Some("../../etc/passwd"), None)).unwrap_err();
        assert!(err.contains("no voice called"), "{err}");
        // An OpenAI client's voice name, with no engine of ours named.
        let planned = plan(&config, &offered, &request(Some("tts-1"), Some("alloy"), None)).unwrap();
        assert_eq!(planned, config);
    }

    #[test]
    fn an_uploaded_voice_is_used_from_that_clients_folder_only() {
        let (config, offered) = base();
        let mine = tempfile::tempdir().unwrap();
        let theirs = tempfile::tempdir().unwrap();
        std::fs::write(mine.path().join("Rick.wav"), b"RIFF").unwrap();

        let planned = plan(&config, &offered, &request(None, Some("custom:Rick"), Some(mine.path()))).unwrap();
        assert_eq!(planned.kitten_tts_2.voice_mode, "clone");
        assert_eq!(planned.kitten_tts_2.cloned_voice, "Rick");
        assert_eq!(planned.kitten_tts_2.voice_dir, mine.path().to_string_lossy());

        let pocket = plan(&config, &offered, &request(Some("pocket_tts"), Some("custom:Rick"), Some(mine.path()))).unwrap();
        assert_eq!(pocket.pocket_tts.voice, "Rick");
        assert_eq!(pocket.pocket_tts.voice_dir, mine.path().to_string_lossy());

        // Another client, no client at all, and a name that is not a name.
        for (voice, folder) in [
            ("custom:Rick", Some(theirs.path())),
            ("custom:Rick", None),
            ("custom:../Rick", Some(mine.path())),
            ("custom:", Some(mine.path())),
        ] {
            let err = plan(&config, &offered, &request(None, Some(voice), folder)).unwrap_err();
            assert!(err.contains("has not been uploaded"), "{voice}: {err}");
        }

        let err = plan(&config, &offered, &request(Some("espeak"), Some("custom:Rick"), Some(mine.path()))).unwrap_err();
        assert!(err.contains("cannot speak with an uploaded voice"), "{err}");
    }

    #[test]
    fn a_pocket_voice_that_is_not_on_disk_is_not_offered() {
        // No Pocket-TTS model or voice files exist where tests run, so none
        // of its built-in voices may be listed: each would fail if picked.
        let config = TtsConfig { engine: TtsEngine::PocketTts, ..Default::default() };
        if voxctrl_tts::POCKET_TTS_VOICES.iter().any(|v| voxctrl_tts::is_pocket_tts_ready(v.id, "")) {
            eprintln!("skipped: Pocket-TTS voices are installed on this machine");
            return;
        }
        let pocket = offer(&config, &TtsEngine::PocketTts);
        assert!(pocket.voices.is_empty(), "{:?}", pocket.voices);
        assert!(pocket.custom_voices, "uploaded voices still work");
        // And a request for one is refused up front, not failed by the engine.
        let err = plan(&config, &[TtsEngine::PocketTts], &request(Some("pocket_tts"), Some("anna"), None)).unwrap_err();
        assert!(err.contains("no voice called"), "{err}");
    }

    #[test]
    fn offers_say_what_a_client_can_pick() {
        let (config, _) = base();
        let kitten = offer(&config, &TtsEngine::KittenTts2);
        assert_eq!(kitten.id, "kitten_tts_2");
        assert_eq!(kitten.name, "Kitten TTS 2");
        assert!(kitten.voices.contains(&"Bruno".to_string()));
        assert!(kitten.custom_voices);
        assert_eq!(kitten.default_voice, Some(config.kitten_tts_2.voice.clone()));

        let espeak = offer(&config, &TtsEngine::Espeak);
        assert!(espeak.voices.is_empty());
        assert!(!espeak.custom_voices);
    }

    #[test]
    fn the_voice_reported_is_the_selected_engines() {
        let mut config = TtsConfig { engine: TtsEngine::Piper, voice: "en-us-amy-low".into(), ..Default::default() };
        assert_eq!(configured_voice(&config).as_deref(), Some("en-us-amy-low"));
        config.engine = TtsEngine::Espeak;
        assert_eq!(configured_voice(&config), None);
        config.engine = TtsEngine::PocketTts;
        assert_eq!(configured_voice(&config), Some(config.pocket_tts.voice.clone()));
    }

    #[tokio::test]
    async fn an_engine_the_server_cannot_run_is_refused_at_startup() {
        for engine in [TtsEngine::InflectMicro, TtsEngine::Server] {
            let err = match EngineSpeaker::prepare(TtsConfig { engine, ..Default::default() }, &[]).await {
                Ok(_) => panic!("accepted"),
                Err(e) => e,
            };
            assert!(err.to_string().contains("cannot run on the server"), "{err}");
        }
    }

    #[test]
    fn the_engine_is_loaded_and_run_on_one_thread_whoever_asks() {
        // The regression this guards: the engine's helper process dies with
        // the thread that started it, so loading and every request must be on
        // the same, lasting thread, not on whichever thread the caller is.
        let threads = std::sync::Arc::new(Mutex::new(Vec::new()));
        let seen = threads.clone();
        let worker = SpeechWorker::start(move || {
            seen.lock().unwrap().push(std::thread::current().id());
            Ok(move |speech: &PlannedSpeech| {
                seen.lock().unwrap().push(std::thread::current().id());
                Ok(speech.text.as_bytes().to_vec())
            })
        })
        .unwrap();
        let worker = std::sync::Arc::new(worker);

        let callers: Vec<_> = (0..4)
            .map(|i| {
                let worker = worker.clone();
                std::thread::spawn(move || worker.speak(say(&format!("request {i}"))).unwrap())
            })
            .collect();
        let mut answers: Vec<String> =
            callers.into_iter().map(|c| String::from_utf8(c.join().unwrap()).unwrap()).collect();
        answers.sort();
        assert_eq!(answers, vec!["request 0", "request 1", "request 2", "request 3"]);

        let threads = threads.lock().unwrap();
        assert_eq!(threads.len(), 5, "one load and four requests");
        assert!(threads.iter().all(|t| *t == threads[0]), "everything ran on the speech thread");
        assert_ne!(threads[0], std::thread::current().id());
    }

    #[test]
    fn a_model_that_will_not_load_fails_startup_with_its_reason() {
        let result = SpeechWorker::start(|| -> Result<fn(&PlannedSpeech) -> Result<Vec<u8>>> {
            Err(anyhow!("the voice file is missing"))
        });
        assert!(result.err().unwrap().to_string().contains("the voice file is missing"));
    }

    #[test]
    fn a_failed_request_does_not_stop_the_next_one() {
        let worker = SpeechWorker::start(|| {
            Ok(|speech: &PlannedSpeech| {
                if speech.text == "bad" { Err(anyhow!("could not say that")) } else { Ok(vec![1, 2, 3]) }
            })
        })
        .unwrap();
        assert!(worker.speak(say("bad")).unwrap_err().to_string().contains("could not say that"));
        assert_eq!(worker.speak(say("good")).unwrap(), vec![1, 2, 3]);
    }

    /// The real thing, end to end, where eSpeak NG is installed.
    #[tokio::test]
    async fn espeak_speaks_a_wav_when_it_is_installed() {
        if voxctrl_config::find_in_path("espeak-ng").is_none() {
            eprintln!("skipped: espeak-ng is not installed");
            return;
        }
        let speaker = EngineSpeaker::prepare(TtsConfig { engine: TtsEngine::Espeak, ..Default::default() }, &[])
            .await
            .unwrap();
        let request = SpeechRequest { text: "Hello from the server.".into(), ..Default::default() };
        let wav = tokio::task::spawn_blocking(move || speaker.speak(&request)).await.unwrap().unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(wav.len() > 4000, "only {} bytes of audio", wav.len());
    }
}
