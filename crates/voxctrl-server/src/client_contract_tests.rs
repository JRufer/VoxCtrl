//! The desktop app's own client code, run against this server over a real
//! socket.
//!
//! The server exists to be used by VoxCtrl's "Remote Speech Engine", so these
//! tests drive the three things that engine does — the Settings "Test
//! Connection" probe, a whole-clip upload, and the upload that streams while
//! the user is still talking — through `voxctrl-inference`'s real HTTP client.
//! If either side changes what it sends or expects, this is where it shows.

use std::sync::Arc;

use voxctrl_config::RemoteOpenAiConfig;
use voxctrl_inference::backend::{TranscribeRequest, TranscriptionBackend};
use voxctrl_inference::{test_remote_speech_engine, RemoteOpenAiBackend, RemoteStreamingSession};

use crate::engine::test_support::{fake_engine, Seen};
use crate::http::{router, AppState};

const KEY: &str = "vx_contract_test_key";

/// Start the server on a free local port; returns its `/v1` endpoint.
async fn serve(text: &str) -> (String, Arc<std::sync::Mutex<Seen>>) {
    let (engine, seen) = fake_engine(text, false);
    serve_with(engine, seen, None).await
}

/// Answers with the style and the text, so a test can see both arrived.
struct EchoCleaner;

impl crate::cleanup::Cleaner for EchoCleaner {
    fn clean(&self, text: &str, styling: &str) -> String {
        format!("<{styling}> {text}")
    }

    fn model(&self) -> &str {
        "echo"
    }
}

async fn serve_with(
    engine: crate::engine::Engine,
    seen: Arc<std::sync::Mutex<Seen>>,
    cleanup: Option<Arc<dyn crate::cleanup::Cleaner>>,
) -> (String, Arc<std::sync::Mutex<Seen>>) {
    serve_all(engine, seen, cleanup, None).await
}

async fn serve_all(
    engine: crate::engine::Engine,
    seen: Arc<std::sync::Mutex<Seen>>,
    cleanup: Option<Arc<dyn crate::cleanup::Cleaner>>,
    speech: Option<Arc<dyn crate::speech::Speaker>>,
) -> (String, Arc<std::sync::Mutex<Seen>>) {
    let voices = speech.is_some().then(|| crate::voices::VoiceStore::new(voice_root()));
    let state = Arc::new(AppState {
        engine,
        cleanup,
        llm: None,
        speech,
        voices,
        api_key: Some(KEY.into()),
        max_upload_bytes: 64 * 1024 * 1024,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router(state)).await });
    (endpoint, seen)
}

fn client_config(endpoint: &str, api_key: Option<&str>) -> RemoteOpenAiConfig {
    RemoteOpenAiConfig {
        endpoint: endpoint.to_string(),
        api_key: api_key.map(str::to_string),
        model: "whisper-1".into(),
        language: "en".into(),
        timeout_secs: 10,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn test_connection_succeeds_and_discovers_the_model() {
    let (endpoint, seen) = serve("unused").await;
    let result = test_remote_speech_engine(&endpoint, Some(KEY), "whisper-1", 10, None).await.unwrap();
    assert!(result.success, "{}", result.message);
    assert_eq!(result.models, vec!["fake/test".to_string()]);
    // The probe is silence, so it must not have cost a model run.
    assert_eq!(seen.lock().unwrap().calls, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_connection_reports_a_wrong_key() {
    let (endpoint, _) = serve("unused").await;
    let result = test_remote_speech_engine(&endpoint, Some("vx_wrong"), "whisper-1", 10, None).await.unwrap();
    assert!(!result.success);
    assert!(result.message.contains("401"), "{}", result.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_whole_clip_upload_is_transcribed() {
    let (endpoint, seen) = serve("whole clip").await;
    let backend = RemoteOpenAiBackend::new(client_config(&endpoint, Some(KEY)));
    let request =
        TranscribeRequest { audio: vec![0.2; 24_000], initial_prompt: Some("Hey Vox".into()), ..Default::default() };
    let result = tokio::task::spawn_blocking(move || backend.transcribe(&request)).await.unwrap().unwrap();
    assert_eq!(result.text, "whole clip");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.samples, 24_000);
    assert_eq!(seen.language.as_deref(), Some("en"));
    assert_eq!(seen.prompt.as_deref(), Some("Hey Vox"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streamed_upload_is_transcribed_in_full() {
    let (endpoint, seen) = serve("streamed while talking").await;
    let session = RemoteStreamingSession::start(
        client_config(&endpoint, Some(KEY)),
        Some("Hey Vox".into()),
        &tokio::runtime::Handle::current(),
    );
    // Ten chunks of 100 ms each, as the audio thread would deliver them.
    for _ in 0..10 {
        session.send_chunk(vec![0.2; 1_600]);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let result = session.finish().await.unwrap();
    assert_eq!(result.text, "streamed while talking");

    let seen = seen.lock().unwrap();
    assert_eq!(seen.calls, 1);
    assert_eq!(seen.samples, 16_000);
    assert_eq!(seen.prompt.as_deref(), Some("Hey Vox"));
}

/// `timeout_secs` is how long the server gets to answer, not how long the
/// user may talk. The streamed upload used to be under it from its first
/// byte, so any dictation longer than the timeout (30 s by default) failed
/// while it was still being spoken.
#[tokio::test(flavor = "multi_thread")]
async fn a_dictation_longer_than_the_timeout_is_transcribed() {
    let (endpoint, seen) = serve("a long dictation").await;
    let mut cfg = client_config(&endpoint, Some(KEY));
    cfg.timeout_secs = 1;
    let session = RemoteStreamingSession::start(cfg, None, &tokio::runtime::Handle::current());
    for _ in 0..25 {
        session.send_chunk(vec![0.2; 1600]);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    let result = session.finish().await.unwrap();
    assert_eq!(result.text, "a long dictation");
    assert_eq!(seen.lock().unwrap().samples, 25 * 1600);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_client_with_no_key_gets_the_servers_explanation() {
    let (endpoint, _) = serve("unused").await;
    let backend = RemoteOpenAiBackend::new(client_config(&endpoint, None));
    // A minute of audio: far more than fits in a socket buffer, so the server
    // has to read it rather than answer early and close the connection.
    let request = TranscribeRequest { audio: vec![0.2; 60 * 16_000], ..Default::default() };
    let err = tokio::task::spawn_blocking(move || backend.transcribe(&request)).await.unwrap().unwrap_err();
    let message = format!("{err:#}");
    assert!(message.contains("401"), "{message}");
    assert!(message.contains("requires an API key"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_streamed_upload_with_a_wrong_key_gets_the_servers_explanation() {
    let (endpoint, seen) = serve("unused").await;
    let session = RemoteStreamingSession::start(
        client_config(&endpoint, Some("vx_wrong")),
        None,
        &tokio::runtime::Handle::current(),
    );
    for _ in 0..20 {
        session.send_chunk(vec![0.2; 16_000]);
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    let message = format!("{:#}", session.finish().await.unwrap_err());
    assert!(message.contains("401"), "{message}");
    assert!(message.contains("was not accepted"), "{message}");
    assert_eq!(seen.lock().unwrap().calls, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn this_app_and_this_server_agree_they_are_compatible() {
    let (endpoint, _) = serve("unused").await;
    // The probe reads /health; a server and app from the same release have
    // nothing to say to each other.
    let notice = voxctrl_inference::probe_remote_speech_engine(&endpoint, Some(KEY), None).await.unwrap();
    assert_eq!(notice, None);

    let health: serde_json::Value = reqwest::get(voxctrl_inference::remote_openai::normalize_health_url(&endpoint))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["contract"].as_u64(), Some(voxctrl_inference::remote_openai::CLIENT_CONTRACT));
    assert_eq!(
        voxctrl_inference::remote_openai::compatibility(&health, voxctrl_inference::remote_openai::APP_VERSION),
        voxctrl_inference::remote_openai::Compatibility::Compatible
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_app_marks_live_passes_and_only_live_passes() {
    use axum::routing::post;
    // A stand-in server that answers every transcription and remembers the
    // purpose header of each.
    let seen: Arc<std::sync::Mutex<Vec<Option<String>>>> = Arc::default();
    let record = seen.clone();
    let app = axum::Router::new().route(
        "/v1/audio/transcriptions",
        post(move |headers: axum::http::HeaderMap, _body: axum::body::Bytes| {
            let record = record.clone();
            async move {
                let purpose = headers
                    .get(voxctrl_inference::remote_openai::PURPOSE_HEADER)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_string);
                record.lock().unwrap().push(purpose);
                axum::Json(serde_json::json!({ "text": "ok" }))
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });

    let backend = Arc::new(RemoteOpenAiBackend::new(client_config(&endpoint, Some(KEY))));
    for interim in [Some(7u64), None] {
        let backend = backend.clone();
        let request = TranscribeRequest { audio: vec![0.2; 16_000], interim_session: interim, ..Default::default() };
        tokio::task::spawn_blocking(move || backend.transcribe(&request)).await.unwrap().unwrap();
    }
    // The dictation itself, streamed.
    let session =
        RemoteStreamingSession::start(client_config(&endpoint, Some(KEY)), None, &tokio::runtime::Handle::current());
    session.send_chunk(vec![0.2; 16_000]);
    session.finish().await.unwrap();

    assert_eq!(*seen.lock().unwrap(), [Some("live".to_string()), None, None]);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_transcription_is_reported_once_not_uploaded_again() {
    // The engine fails every request: the server answers 500.
    let (engine, seen) = fake_engine("unused", true);
    let (endpoint, seen) = serve_with(engine, seen, None).await;
    let session =
        RemoteStreamingSession::start(client_config(&endpoint, Some(KEY)), None, &tokio::runtime::Handle::current());
    session.send_chunk(vec![0.2; 16_000]);
    let message = format!("{:#}", session.finish().await.unwrap_err());
    assert!(message.contains("HTTP 500"), "{message}");
    // Before 0.9.19 the whole recording was sent a second time as a plain
    // upload, and the server transcribed (and failed) it twice.
    assert_eq!(seen.lock().unwrap().calls, 1);
}

// ── Cleanup ───────────────────────────────────────────────────────────────────

use voxctrl_inference::remote_cleanup::{check_server_cleanup, clean_on_server};

async fn serve_cleanup(enabled: bool) -> String {
    let (engine, seen) = fake_engine("unused", false);
    let cleaner: Option<Arc<dyn crate::cleanup::Cleaner>> = enabled.then(|| Arc::new(EchoCleaner) as _);
    serve_with(engine, seen, cleaner).await.0
}

#[tokio::test(flavor = "multi_thread")]
async fn the_client_gets_its_text_cleaned_in_the_style_it_asked_for() {
    let endpoint = serve_cleanup(true).await;
    let cleaned = clean_on_server(&client_config(&endpoint, Some(KEY)), "um hello there", "casual").await.unwrap();
    assert_eq!(cleaned, "<casual> um hello there");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_client_learns_whether_the_server_offers_cleanup() {
    let on = check_server_cleanup(&serve_cleanup(true).await, Some(KEY), None).await;
    assert!(on.available, "{}", on.message);
    assert_eq!(on.model.as_deref(), Some("echo"));

    let off = check_server_cleanup(&serve_cleanup(false).await, Some(KEY), None).await;
    assert!(!off.available);
    assert!(off.message.contains("turned off"), "{}", off.message);

    let wrong_key = check_server_cleanup(&serve_cleanup(true).await, Some("vx_wrong"), None).await;
    assert!(!wrong_key.available);
    assert!(wrong_key.message.contains("API key"), "{}", wrong_key.message);

    let nobody = check_server_cleanup("http://127.0.0.1:1/v1", Some(KEY), None).await;
    assert!(!nobody.available);
    assert!(nobody.message.contains("connect"), "{}", nobody.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_cleanup_the_server_cannot_do_is_an_error_carrying_its_reason() {
    // The caller falls back to the raw transcript on any error, so each of
    // these must be one rather than an empty or half-right string.
    let off = clean_on_server(&client_config(&serve_cleanup(false).await, Some(KEY)), "hello", "casual").await;
    let message = format!("{:#}", off.unwrap_err());
    assert!(message.contains("HTTP 501") && message.contains("not enabled"), "{message}");

    let wrong_key =
        clean_on_server(&client_config(&serve_cleanup(true).await, Some("vx_wrong")), "hello", "casual").await;
    assert!(format!("{:#}", wrong_key.unwrap_err()).contains("HTTP 401"));

    let nobody = clean_on_server(&client_config("http://127.0.0.1:1/v1", Some(KEY)), "hello", "casual").await;
    assert!(nobody.is_err());
}

// ── Speech ────────────────────────────────────────────────────────────────────

use voxctrl_tts::remote::{check_server_tts, fetch_speech, local_custom_voices, sync_voices, ServerLink};

const ALICE: &str = "0123456789abcdef0123456789abcdef";
const BOB: &str = "fedcba9876543210fedcba9876543210";

/// A fresh folder per server, kept for the life of the test process.
fn voice_root() -> std::path::PathBuf {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("client-voices");
    std::mem::forget(dir);
    path
}

/// A valid, tiny WAV. Its sample count is the length of a line describing
/// the request as the engine would see it, and that line is kept so a test
/// can read it back.
#[derive(Default)]
struct DescribingSpeaker(std::sync::Mutex<Vec<String>>);

impl crate::speech::Speaker for DescribingSpeaker {
    fn speak(&self, request: &crate::speech::SpeechRequest) -> Result<Vec<u8>, crate::speech::SpeakError> {
        // As the real speaker does: an uploaded voice must be in the folder
        // of the client that is asking.
        if let Some(custom) = request.voice.as_deref().and_then(|v| v.strip_prefix("custom:")) {
            let found = request.client_voices.as_ref().is_some_and(|dir| dir.join(format!("{custom}.wav")).is_file());
            if !found {
                return Err(crate::speech::SpeakError::Invalid(format!(
                    "The voice \"{custom}\" has not been uploaded to this server by this client."
                )));
            }
        }
        // A slow engine: busy until the client gives up, or five seconds.
        if request.text == "take your time" {
            let started = std::time::Instant::now();
            while !request.abandoned.load(std::sync::atomic::Ordering::SeqCst)
                && started.elapsed() < std::time::Duration::from_secs(5)
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            let gone = request.abandoned.load(std::sync::atomic::Ordering::SeqCst);
            self.0.lock().unwrap().push(format!("slow request ended, client gone: {gone}"));
            return Err(crate::speech::SpeakError::Abandoned);
        }
        let line = format!(
            "{}|{}|{}",
            request.engine.as_deref().unwrap_or("-"),
            request.voice.as_deref().unwrap_or("-"),
            request.text
        );
        let samples: Vec<u8> = (0..line.chars().count() as i16).flat_map(|s| s.to_le_bytes()).collect();
        self.0.lock().unwrap().push(line);
        Ok(voxctrl_tts::synth::pcm16_to_wav(&samples, 16000))
    }

    fn engines(&self) -> Vec<crate::speech::EngineOffer> {
        vec![
            crate::speech::EngineOffer {
                id: "kitten_tts_2".into(),
                name: "Kitten TTS 2".into(),
                voices: vec!["Bruno".into(), "Luna".into()],
                default_voice: Some("Bruno".into()),
                custom_voices: true,
            },
            crate::speech::EngineOffer {
                id: "piper".into(),
                name: "Piper".into(),
                voices: vec!["en-us-amy-low".into()],
                default_voice: Some("en-us-amy-low".into()),
                custom_voices: false,
            },
        ]
    }
}

async fn serve_speech(enabled: bool) -> (String, Arc<DescribingSpeaker>) {
    let (engine, seen) = fake_engine("unused", false);
    let speaker = Arc::new(DescribingSpeaker::default());
    let offered: Option<Arc<dyn crate::speech::Speaker>> = enabled.then(|| speaker.clone() as _);
    (serve_all(engine, seen, None, offered).await.0, speaker)
}

fn link(endpoint: &str, key: &str) -> ServerLink {
    ServerLink { endpoint: endpoint.to_string(), api_key: Some(key.to_string()), ..Default::default() }
}

fn link_as(endpoint: &str, client: &str, engine: Option<&str>, voice: Option<&str>) -> ServerLink {
    ServerLink {
        endpoint: endpoint.to_string(),
        api_key: Some(KEY.to_string()),
        client_id: client.to_string(),
        tls_fingerprint: None,
        engine: engine.map(str::to_string),
        voice: voice.map(str::to_string),
    }
}

/// A folder of voice clips as the app keeps them.
fn voice_folder(voices: &[(&str, Option<&str>)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (name, transcript) in voices {
        let clip = voxctrl_tts::synth::pcm16_to_wav(&[1, 0, 2, 0, 3, 0], 16000);
        std::fs::write(dir.path().join(format!("{name}.wav")), clip).unwrap();
        if let Some(transcript) = transcript {
            std::fs::write(dir.path().join(format!("{name}.txt")), transcript).unwrap();
        }
    }
    dir
}

// The app's speech client is blocking (it runs on the TTS worker thread), so
// these drive it from a blocking task, as the worker would.

#[tokio::test(flavor = "multi_thread")]
async fn the_client_gets_a_wav_it_can_play() {
    let (endpoint, speaker) = serve_speech(true).await;
    let wav =
        tokio::task::spawn_blocking(move || fetch_speech(&link(&endpoint, KEY), "Hello there")).await.unwrap().unwrap();
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(*speaker.0.lock().unwrap(), vec!["-|-|Hello there"], "nothing chosen: the server's defaults");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_clients_choice_of_engine_and_voice_reaches_the_server() {
    let (endpoint, speaker) = serve_speech(true).await;
    tokio::task::spawn_blocking(move || {
        fetch_speech(&link_as(&endpoint, ALICE, Some("kitten_tts_2"), Some("Luna")), "hi").unwrap()
    })
    .await
    .unwrap();
    assert_eq!(*speaker.0.lock().unwrap(), vec!["kitten_tts_2|Luna|hi"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_client_learns_whether_and_how_the_server_speaks() {
    let ((on, _), (off, _)) = (serve_speech(true).await, serve_speech(false).await);
    let (on, off, wrong_key) = tokio::task::spawn_blocking(move || {
        (
            check_server_tts(&link(&on, KEY)),
            check_server_tts(&link(&off, KEY)),
            check_server_tts(&link(&on, "vx_wrong")),
        )
    })
    .await
    .unwrap();
    assert!(on.available, "{}", on.message);
    assert_eq!(on.message, "This server offers 2 speech engines.");
    assert_eq!(on.engines.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), vec!["kitten_tts_2", "piper"]);
    assert_eq!(on.engines[0].voices, vec!["Bruno", "Luna"]);
    assert!(on.engines[0].custom_voices && !on.engines[1].custom_voices);
    assert!(on.custom_voices);
    assert!(!off.available);
    assert!(off.message.contains("turned off"), "{}", off.message);
    assert!(wrong_key.message.contains("API key"), "{}", wrong_key.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn speech_the_server_cannot_give_is_an_error_the_user_can_act_on() {
    let ((on, _), (off, _)) = (serve_speech(true).await, serve_speech(false).await);
    let (disabled, wrong_key, not_uploaded) = tokio::task::spawn_blocking(move || {
        (
            fetch_speech(&link(&off, KEY), "hi"),
            fetch_speech(&link(&on, "vx_wrong"), "hi"),
            fetch_speech(&link_as(&on, ALICE, None, Some("custom:Rick")), "hi"),
        )
    })
    .await
    .unwrap();
    let message = disabled.unwrap_err().to_string();
    assert!(message.contains("HTTP 501") && message.contains("not enabled"), "{message}");
    assert!(wrong_key.unwrap_err().to_string().contains("API key"));
    let message = not_uploaded.unwrap_err().to_string();
    assert!(message.contains("HTTP 400") && message.contains("has not been uploaded"), "{message}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clients_own_voices_are_uploaded_used_kept_private_and_removed() {
    let (endpoint, speaker) = serve_speech(true).await;
    tokio::task::spawn_blocking(move || {
        let alice = link_as(&endpoint, ALICE, Some("kitten_tts_2"), Some("custom:Rick"));
        let bob = link_as(&endpoint, BOB, Some("kitten_tts_2"), Some("custom:Rick"));
        let folder = voice_folder(&[("Rick", Some("hello there")), ("my voice", None)]);

        // First sync uploads both; a second has nothing to do.
        let first = sync_voices(&alice, &local_custom_voices(folder.path())).unwrap();
        assert_eq!(first.uploaded, vec!["Rick", "my voice"]);
        assert!(first.removed.is_empty());
        let again = sync_voices(&alice, &local_custom_voices(folder.path())).unwrap();
        assert!(again.uploaded.is_empty() && again.removed.is_empty(), "{again:?}");

        // The uploader can speak with it. Someone else cannot, and has none.
        assert!(fetch_speech(&alice, "hi").is_ok());
        let err = fetch_speech(&bob, "hi").unwrap_err().to_string();
        assert!(err.contains("has not been uploaded"), "{err}");
        assert_eq!(
            sync_voices(&bob, &[]).unwrap(),
            Default::default(),
            "nothing of Bob's to remove, nothing of Alice's touched"
        );
        assert!(fetch_speech(&alice, "still mine").is_ok());

        // A changed transcript is a new version: only that voice goes up again.
        std::fs::write(folder.path().join("Rick.txt"), "different words").unwrap();
        let changed = sync_voices(&alice, &local_custom_voices(folder.path())).unwrap();
        assert_eq!(changed.uploaded, vec!["Rick"]);

        // Deleted on the client: gone from the server at the next sync.
        std::fs::remove_file(folder.path().join("Rick.wav")).unwrap();
        let after_delete = sync_voices(&alice, &local_custom_voices(folder.path())).unwrap();
        assert_eq!(after_delete.removed, vec!["Rick"]);
        assert!(after_delete.uploaded.is_empty());
        let err = fetch_speech(&alice, "hi").unwrap_err().to_string();
        assert!(err.contains("has not been uploaded"), "{err}");
        let other = link_as(&endpoint, ALICE, Some("kitten_tts_2"), Some("custom:my voice"));
        assert!(fetch_speech(&other, "the other one is still there").is_ok());
    })
    .await
    .unwrap();
    let said = speaker.0.lock().unwrap();
    assert_eq!(said.len(), 3, "{said:?}");
    assert_eq!(said[0], "kitten_tts_2|custom:Rick|hi");
}

#[tokio::test(flavor = "multi_thread")]
async fn voices_cannot_be_synced_without_a_client_id() {
    let (endpoint, _) = serve_speech(true).await;
    let err = tokio::task::spawn_blocking(move || sync_voices(&link(&endpoint, KEY), &[])).await.unwrap().unwrap_err();
    assert!(err.to_string().contains("no client id"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn speech_the_user_stopped_is_given_up_at_once_and_the_server_is_told() {
    use voxctrl_tts::remote::fetch_speech_unless;
    let (endpoint, speaker) = serve_speech(true).await;
    let (outcome, waited) = tokio::task::spawn_blocking(move || {
        let started = std::time::Instant::now();
        // The user stops the speech 300 ms in.
        let stopped = || started.elapsed() > std::time::Duration::from_millis(300);
        let outcome = fetch_speech_unless(&link(&endpoint, KEY), "take your time", &stopped);
        (outcome, started.elapsed())
    })
    .await
    .unwrap();
    assert_eq!(outcome.unwrap(), None, "given up, not an error");
    assert!(waited < std::time::Duration::from_secs(2), "the client waited {waited:?} for speech nobody wanted");

    // The server learns of it from the closed connection.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
    loop {
        if let Some(line) = speaker.0.lock().unwrap().first().cloned() {
            assert_eq!(line, "slow request ended, client gone: true");
            break;
        }
        assert!(std::time::Instant::now() < deadline, "the server never noticed the client had gone");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn speech_that_is_not_stopped_still_arrives_through_the_cancellable_path() {
    use voxctrl_tts::remote::fetch_speech_unless;
    let (endpoint, _) = serve_speech(true).await;
    let wav = tokio::task::spawn_blocking(move || fetch_speech_unless(&link(&endpoint, KEY), "hello", &|| false))
        .await
        .unwrap()
        .unwrap()
        .expect("not cancelled");
    assert_eq!(&wav[0..4], b"RIFF");
}

// ── Encryption ──────────────────────────────────────────────────────────────

/// A server with encryption on, cleanup and speech included; returns its
/// `https` endpoint and the fingerprint of its certificate.
async fn serve_encrypted() -> (String, String) {
    let (engine, _seen) = fake_engine("spoken over an encrypted connection", false);
    let speaker: Arc<dyn crate::speech::Speaker> = Arc::new(DescribingSpeaker::default());
    let state = Arc::new(AppState {
        engine,
        cleanup: Some(Arc::new(EchoCleaner)),
        llm: None,
        voices: Some(crate::voices::VoiceStore::new(voice_root())),
        speech: Some(speaker),
        api_key: Some(KEY.into()),
        max_upload_bytes: 64 * 1024 * 1024,
    });
    let dir = tempfile::tempdir().unwrap();
    let (identity, _) = crate::tls::Identity::load_or_create(dir.path()).unwrap();
    let fingerprint = identity.fingerprint();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("https://{}/v1", listener.local_addr().unwrap());
    let tls = axum_server::tls_rustls::RustlsConfig::from_config(Arc::new(identity.server_config().unwrap()));
    tokio::spawn(async move {
        crate::plain_http::https_server(listener, tls).serve(router(state).into_make_service()).await
    });
    (endpoint, fingerprint)
}

fn paired(endpoint: &str, fingerprint: &str) -> RemoteOpenAiConfig {
    RemoteOpenAiConfig { tls_fingerprint: Some(fingerprint.to_string()), ..client_config(endpoint, Some(KEY)) }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paired_client_does_everything_over_the_encrypted_connection() {
    let (endpoint, fingerprint) = serve_encrypted().await;

    let test = test_remote_speech_engine(&endpoint, Some(KEY), "whisper-1", 10, Some(&fingerprint)).await.unwrap();
    assert!(test.success, "{}", test.message);

    let backend = RemoteOpenAiBackend::new(paired(&endpoint, &fingerprint));
    let request = TranscribeRequest { audio: vec![0.2; 16_000], ..Default::default() };
    let heard = tokio::task::spawn_blocking(move || backend.transcribe(&request)).await.unwrap().unwrap();
    assert_eq!(heard.text, "spoken over an encrypted connection");

    let cleaned = clean_on_server(&paired(&endpoint, &fingerprint), "um hello", "casual").await.unwrap();
    assert_eq!(cleaned, "<casual> um hello");
    assert!(check_server_cleanup(&endpoint, Some(KEY), Some(&fingerprint)).await.available);

    let speech_link = ServerLink { tls_fingerprint: Some(fingerprint.clone()), ..link(&endpoint, KEY) };
    let (status, wav) =
        tokio::task::spawn_blocking(move || (check_server_tts(&speech_link), fetch_speech(&speech_link, "hello")))
            .await
            .unwrap();
    assert!(status.available, "{}", status.message);
    assert!(wav.unwrap().starts_with(b"RIFF"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_different_certificate_is_refused_and_the_reason_is_given() {
    let (endpoint, _real) = serve_encrypted().await;
    let other = "ab".repeat(32);

    let test = test_remote_speech_engine(&endpoint, Some(KEY), "whisper-1", 10, Some(&other)).await.unwrap();
    assert!(!test.success);

    let reason = voxctrl_inference::probe_remote_speech_engine(&endpoint, Some(KEY), Some(&other))
        .await
        .unwrap_err()
        .to_string();
    assert_eq!(reason, voxctrl_pairing::CERTIFICATE_MISMATCH_HELP);

    let cleanup = check_server_cleanup(&endpoint, Some(KEY), Some(&other)).await;
    assert_eq!(cleanup.message, voxctrl_pairing::CERTIFICATE_MISMATCH_HELP);

    let speech_link = ServerLink { tls_fingerprint: Some(other), ..link(&endpoint, KEY) };
    let (status, spoken) =
        tokio::task::spawn_blocking(move || (check_server_tts(&speech_link), fetch_speech(&speech_link, "hello")))
            .await
            .unwrap();
    assert_eq!(status.message, voxctrl_pairing::CERTIFICATE_MISMATCH_HELP);
    assert_eq!(spoken.unwrap_err().to_string(), voxctrl_pairing::CERTIFICATE_MISMATCH_HELP);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unpaired_client_does_not_trust_the_servers_own_certificate() {
    let (endpoint, _fingerprint) = serve_encrypted().await;
    let reason =
        voxctrl_inference::probe_remote_speech_engine(&endpoint, Some(KEY), None).await.unwrap_err().to_string();
    assert!(reason.contains("Could not connect"), "{reason}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_paired_client_never_falls_back_to_an_unencrypted_address() {
    let (plain, _seen) = serve("sent in the clear").await;
    let fingerprint = "ab".repeat(32);

    let reason = voxctrl_inference::probe_remote_speech_engine(&plain, Some(KEY), Some(&fingerprint))
        .await
        .unwrap_err()
        .to_string();
    assert!(reason.contains("must start with https://"), "{reason}");

    let backend = RemoteOpenAiBackend::new(paired(&plain, &fingerprint));
    let request = TranscribeRequest { audio: vec![0.0; 16_000], ..Default::default() };
    let refused = tokio::task::spawn_blocking(move || backend.transcribe(&request)).await.unwrap().unwrap_err();
    assert!(format!("{refused:#}").contains("must start with https://"), "{refused:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn plain_http_to_an_encrypted_server_is_told_to_pair() {
    let (endpoint, _fingerprint) = serve_encrypted().await;
    let plain = endpoint.replacen("https://", "http://", 1);

    // The background check: the reason the app shows beside the offline mark.
    let reason = voxctrl_inference::probe_remote_speech_engine(&plain, Some(KEY), None).await.unwrap_err().to_string();
    assert!(reason.contains("pairing code"), "{reason}");

    // Test connection in Settings shows the server's own words.
    let test = test_remote_speech_engine(&plain, Some(KEY), "whisper-1", 10, None).await.unwrap();
    assert!(!test.success);
    assert!(test.message.contains("pairing code"), "{}", test.message);

    // A dictation fails once with that message; it is not uploaded twice.
    let session =
        RemoteStreamingSession::start(client_config(&plain, Some(KEY)), None, &tokio::runtime::Handle::current());
    session.send_chunk(vec![0.1; 16_000]);
    let failed = session.finish().await.unwrap_err();
    assert!(format!("{failed:#}").contains("pairing code"), "{failed:#}");
    assert!(format!("{failed:#}").contains("HTTP 426"), "{failed:#}");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_health_check_speaks_to_an_encrypted_server() {
    let (endpoint, fingerprint) = serve_encrypted().await;
    let bind = endpoint.trim_start_matches("https://").trim_end_matches("/v1").to_string();
    let (good, bad) = tokio::task::spawn_blocking(move || {
        (crate::healthcheck(&bind, Some(&fingerprint)), crate::healthcheck(&bind, Some(&"ab".repeat(32))))
    })
    .await
    .unwrap();
    assert!(good.is_ok(), "{good:?}");
    assert!(bad.is_err());
}

// ── AI processing ───────────────────────────────────────────────────────────

/// A server whose AI processing is answered by a stand-in language model
/// service; returns the server's endpoint and what that service saw.
async fn serve_with_llm(model: &str) -> (String, Arc<crate::llm::testing::Seen>) {
    let (service, seen) = crate::llm::testing::fake_service(&["llama3.2:1b"]).await;
    let relay = crate::llm::LlmRelay::prepare(&crate::config::LlmConfig {
        enabled: true,
        endpoint: service,
        api_key: "sk-kept-on-the-server".into(),
        model: model.into(),
        timeout_secs: 10,
    })
    .await
    .unwrap();
    let (engine, _) = fake_engine("unused", false);
    let state = Arc::new(AppState {
        engine,
        cleanup: None,
        llm: Some(Arc::new(relay)),
        speech: None,
        voices: None,
        api_key: Some(KEY.into()),
        max_upload_bytes: 64 * 1024 * 1024,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router(state)).await });
    (endpoint, seen)
}

fn ai_settings() -> voxctrl_config::OpenAiConfig {
    voxctrl_config::OpenAiConfig {
        enabled: true,
        on_server: true,
        // The client's own service settings, none of which may be used.
        endpoint: "http://127.0.0.1:9".into(),
        api_key: Some("sk-the-clients-own".into()),
        model: "the-clients-model".into(),
        system_prompt: "Shout.".into(),
        user_prompt: "{text}".into(),
        timeout_secs: 10,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_apps_ai_processing_goes_through_the_server_to_its_model() {
    let (endpoint, seen) = serve_with_llm("llama3.2:1b").await;

    let status = voxctrl_llm::check_server_llm(&endpoint, Some(KEY), None).await;
    assert!(status.available, "{}", status.message);
    assert_eq!(status.model.as_deref(), Some("llama3.2:1b"));

    let client = voxctrl_llm::OpenAiClient::via_server(ai_settings(), &client_config(&endpoint, Some(KEY)));
    assert_eq!(client.process("hello there").await, "[llama3.2:1b] HELLO THERE");

    // The service saw the client's prompts, the server's model and the
    // server's key: never the client's own model, nor the VoxCtrl key.
    let sent = seen.requests.lock().unwrap()[0].clone();
    assert_eq!(sent["model"], "llama3.2:1b");
    assert_eq!(sent["messages"][0]["content"], "Shout.");
    assert_eq!(sent["messages"][1]["content"], "hello there");
    assert_eq!(seen.keys.lock().unwrap()[0].as_deref(), Some("Bearer sk-kept-on-the-server"));
}

#[tokio::test(flavor = "multi_thread")]
async fn ai_processing_that_cannot_happen_leaves_the_text_as_it_was_said() {
    // The server's model is one its service does not have.
    let (endpoint, _seen) = serve_with_llm("missing").await;
    let client = voxctrl_llm::OpenAiClient::via_server(ai_settings(), &client_config(&endpoint, Some(KEY)));
    assert_eq!(client.process("hello there").await, "hello there");

    // A wrong key for the VoxCtrl server.
    let (endpoint, seen) = serve_with_llm("llama3.2:1b").await;
    let client = voxctrl_llm::OpenAiClient::via_server(ai_settings(), &client_config(&endpoint, Some("vx_wrong")));
    assert_eq!(client.process("hello there").await, "hello there");
    assert!(seen.requests.lock().unwrap().is_empty());
    assert!(voxctrl_llm::check_server_llm(&endpoint, Some("vx_wrong"), None).await.message.contains("API key"));

    // A server with AI processing off.
    let (off, _) = serve("unused").await;
    let client = voxctrl_llm::OpenAiClient::via_server(ai_settings(), &client_config(&off, Some(KEY)));
    assert_eq!(client.process("hello there").await, "hello there");
    let status = voxctrl_llm::check_server_llm(&off, Some(KEY), None).await;
    assert!(!status.available);
    assert!(status.message.contains("turned off on this server"), "{}", status.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hotkeys_ai_step_follows_the_use_the_server_setting() {
    let (endpoint, seen) = serve_with_llm("llama3.2:1b").await;
    let mut app = voxctrl_config::AppConfig { openai: ai_settings(), ..Default::default() };
    app.engine.remote_openai = client_config(&endpoint, Some(KEY));
    let binding: voxctrl_routing::HotkeyBinding = serde_json::from_value(serde_json::json!({
        "id": "b", "keys": ["KEY_F9"], "gesture": "hold", "target_id": "default",
        "openai_enabled": true, "openai_model": "the-hotkeys-model",
    }))
    .unwrap();

    let client = voxctrl_inference::finalize::binding_llm_client(Some(&binding), &app).unwrap();
    assert_eq!(client.process("hello").await, "[llama3.2:1b] HELLO");
    assert_eq!(seen.requests.lock().unwrap().len(), 1);

    // Off: the same step goes to the client's own service, not the server.
    app.openai.on_server = false;
    let client = voxctrl_inference::finalize::binding_llm_client(Some(&binding), &app).unwrap();
    assert_eq!(client.process("hello").await, "hello");
    assert_eq!(seen.requests.lock().unwrap().len(), 1);
}
