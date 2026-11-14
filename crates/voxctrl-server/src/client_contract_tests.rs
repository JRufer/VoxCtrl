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
    let state = Arc::new(AppState { engine, cleanup, api_key: Some(KEY.into()), max_upload_bytes: 64 * 1024 * 1024 });
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
    let result = test_remote_speech_engine(&endpoint, Some(KEY), "whisper-1", 10).await.unwrap();
    assert!(result.success, "{}", result.message);
    assert_eq!(result.models, vec!["fake/test".to_string()]);
    // The probe is silence, so it must not have cost a model run.
    assert_eq!(seen.lock().unwrap().calls, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_connection_reports_a_wrong_key() {
    let (endpoint, _) = serve("unused").await;
    let result = test_remote_speech_engine(&endpoint, Some("vx_wrong"), "whisper-1", 10).await.unwrap();
    assert!(!result.success);
    assert!(result.message.contains("401"), "{}", result.message);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_whole_clip_upload_is_transcribed() {
    let (endpoint, seen) = serve("whole clip").await;
    let backend = RemoteOpenAiBackend::new(client_config(&endpoint, Some(KEY)));
    let request = TranscribeRequest {
        audio: vec![0.2; 24_000],
        initial_prompt: Some("Hey Vox".into()),
        ..Default::default()
    };
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
    let on = check_server_cleanup(&serve_cleanup(true).await, Some(KEY)).await;
    assert!(on.available, "{}", on.message);
    assert_eq!(on.model.as_deref(), Some("echo"));

    let off = check_server_cleanup(&serve_cleanup(false).await, Some(KEY)).await;
    assert!(!off.available);
    assert!(off.message.contains("turned off"), "{}", off.message);

    let wrong_key = check_server_cleanup(&serve_cleanup(true).await, Some("vx_wrong")).await;
    assert!(!wrong_key.available);
    assert!(wrong_key.message.contains("API key"), "{}", wrong_key.message);

    let nobody = check_server_cleanup("http://127.0.0.1:1/v1", Some(KEY)).await;
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

    let wrong_key = clean_on_server(&client_config(&serve_cleanup(true).await, Some("vx_wrong")), "hello", "casual").await;
    assert!(format!("{:#}", wrong_key.unwrap_err()).contains("HTTP 401"));

    let nobody = clean_on_server(&client_config("http://127.0.0.1:1/v1", Some(KEY)), "hello", "casual").await;
    assert!(nobody.is_err());
}
