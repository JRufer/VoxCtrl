//! Text-to-speech on the server: the engine and voice set under `[tts]` in
//! `server.toml`, for clients whose TTS engine is "VoxCtrl server".
//!
//! The server synthesizes and returns a WAV file; the client plays it. A
//! client's own speed setting and stop key still apply, since playback is
//! theirs.

use std::sync::mpsc;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use tracing::info;
use voxctrl_config::{TtsConfig, TtsEngine};
use voxctrl_tts::synth::{self, Synthesizer};

/// Something that can turn text into a WAV file. A trait so the HTTP layer
/// can be tested without an engine installed.
pub trait Speaker: Send + Sync {
    /// Blocking, and can take seconds: call it off the async runtime.
    fn speak(&self, text: &str) -> Result<Vec<u8>>;
    /// The engine's name as config files spell it, e.g. `piper`.
    fn engine(&self) -> String;
    /// The voice it speaks with, where the engine has named voices.
    fn voice(&self) -> Option<String>;
}

/// One of the app's own TTS engines, with its model kept loaded.
pub struct EngineSpeaker {
    config: TtsConfig,
    worker: SpeechWorker,
}

impl EngineSpeaker {
    /// Download what the engine and voice need, and load the model, so the
    /// first request pays for neither.
    pub async fn prepare(config: TtsConfig) -> Result<Self> {
        if !synth::supports(&config.engine) {
            anyhow::bail!(
                "tts.engine = \"{}\" cannot run on the server. Use piper, espeak, pocket_tts, breeze_tts_2, \
                 vox_cpm_2 or kitten_tts_2.",
                synth::engine_id(&config.engine)
            );
        }
        info!("Preparing text-to-speech ({})", synth::engine_id(&config.engine));
        synth::ensure_assets(&config).await.context(
            "Could not get the text-to-speech engine ready. Check this machine's internet access, \
             or turn text-to-speech off (tts.enabled = false).",
        )?;
        let worker_config = config.clone();
        let worker = tokio::task::spawn_blocking(move || {
            SpeechWorker::start(move || {
                let mut synthesizer = Synthesizer::new();
                synthesizer.load(&worker_config)?;
                Ok(move |text: &str| synthesizer.synthesize(&worker_config, text, None))
            })
        })
        .await
        .context("Loading the text-to-speech model stopped unexpectedly")?
        .context("Could not load the text-to-speech model")?;
        Ok(Self { config, worker })
    }
}

struct Job {
    text: String,
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
        S: FnMut(&str) -> Result<Vec<u8>>,
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
                    let _ = job.reply.send(speak(&job.text));
                }
            })
            .context("Could not start the speech thread")?;
        loaded.recv().map_err(|_| anyhow!("The speech thread stopped while loading the model"))??;
        Ok(Self { jobs: Mutex::new(jobs) })
    }

    /// Blocking: waits its turn, then for the audio.
    fn speak(&self, text: &str) -> Result<Vec<u8>> {
        let (reply, answer) = mpsc::channel();
        // Held to the end, so requests are answered in the order they came.
        let jobs = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        jobs.send(Job { text: text.to_string(), reply })
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
    fn speak(&self, text: &str) -> Result<Vec<u8>> {
        self.worker.speak(text)
    }

    fn engine(&self) -> String {
        synth::engine_id(&self.config.engine)
    }

    fn voice(&self) -> Option<String> {
        configured_voice(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            let err = match EngineSpeaker::prepare(TtsConfig { engine, ..Default::default() }).await {
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
            Ok(move |text: &str| {
                seen.lock().unwrap().push(std::thread::current().id());
                Ok(text.as_bytes().to_vec())
            })
        })
        .unwrap();
        let worker = std::sync::Arc::new(worker);

        let callers: Vec<_> = (0..4)
            .map(|i| {
                let worker = worker.clone();
                std::thread::spawn(move || worker.speak(&format!("request {i}")).unwrap())
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
        let result = SpeechWorker::start(|| -> Result<fn(&str) -> Result<Vec<u8>>> {
            Err(anyhow!("the voice file is missing"))
        });
        assert!(result.err().unwrap().to_string().contains("the voice file is missing"));
    }

    #[test]
    fn a_failed_request_does_not_stop_the_next_one() {
        let worker = SpeechWorker::start(|| {
            Ok(|text: &str| if text == "bad" { Err(anyhow!("could not say that")) } else { Ok(vec![1, 2, 3]) })
        })
        .unwrap();
        assert!(worker.speak("bad").unwrap_err().to_string().contains("could not say that"));
        assert_eq!(worker.speak("good").unwrap(), vec![1, 2, 3]);
    }

    /// The real thing, end to end, where eSpeak NG is installed.
    #[tokio::test]
    async fn espeak_speaks_a_wav_when_it_is_installed() {
        if voxctrl_config::find_in_path("espeak-ng").is_none() {
            eprintln!("skipped: espeak-ng is not installed");
            return;
        }
        let speaker = EngineSpeaker::prepare(TtsConfig { engine: TtsEngine::Espeak, ..Default::default() })
            .await
            .unwrap();
        let wav = tokio::task::spawn_blocking(move || speaker.speak("Hello from the server.")).await.unwrap().unwrap();
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert!(wav.len() > 4000, "only {} bytes of audio", wav.len());
    }
}
