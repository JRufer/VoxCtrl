//! The HTTP API: the OpenAI-compatible subset a VoxCtrl client speaks.
//!
//! | Route | Auth | Purpose |
//! |---|---|---|
//! | `GET /health` | none | Liveness, for Docker and monitoring |
//! | `GET /v1/models` | key | The model this server runs |
//! | `POST /v1/audio/transcriptions` | key | Speech to text |
//!
//! Matching OpenAI's shapes is what lets the desktop app's existing "Remote
//! OpenAI" engine use this server unchanged, and lets any other client that
//! speaks the same API use it too.

use std::sync::Arc;

use axum::extract::multipart::MultipartError;
use axum::extract::{DefaultBodyLimit, Multipart, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde_json::json;
use tracing::{info, warn};

use crate::audio;
use crate::engine::Engine;

pub struct AppState {
    pub engine: Engine,
    /// `None` when the server was told not to require a key.
    pub api_key: Option<String>,
    /// Largest request body accepted, in bytes.
    pub max_upload_bytes: usize,
}

pub fn router(state: Arc<AppState>) -> Router {
    let max_upload_bytes = state.max_upload_bytes;
    let protected = Router::new()
        .route("/v1/models", get(models))
        .route("/v1/audio/transcriptions", post(transcriptions))
        .route_layer(middleware::from_fn_with_state(state.clone(), require_key))
        // axum's default cap is 2 MB — about a minute of speech.
        .layer(DefaultBodyLimit::max(max_upload_bytes));

    Router::new()
        .route("/", get(root))
        .route("/health", get(health))
        .merge(protected)
        .with_state(state)
}

// ── Errors ────────────────────────────────────────────────────────────────────

/// An error in OpenAI's envelope: `{"error": {"message", "type"}}`. The
/// VoxCtrl client shows `message` to the user as is, so each one says what to do.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    kind: &'static str,
    message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self { status: StatusCode::BAD_REQUEST, kind: "invalid_request_error", message: message.into() }
    }

    fn unauthorized(message: impl Into<String>) -> Self {
        Self { status: StatusCode::UNAUTHORIZED, kind: "authentication_error", message: message.into() }
    }

    fn server(message: impl Into<String>) -> Self {
        Self { status: StatusCode::INTERNAL_SERVER_ERROR, kind: "server_error", message: message.into() }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "error": { "message": self.message, "type": self.kind } }));
        (self.status, body).into_response()
    }
}

impl From<MultipartError> for ApiError {
    fn from(err: MultipartError) -> Self {
        let status = err.status();
        let message = if status == StatusCode::PAYLOAD_TOO_LARGE {
            "The upload is larger than this server accepts. Raise max_upload_mb in server.toml \
             (or VOXCTRL_SERVER_MAX_UPLOAD_MB) and restart the server."
                .to_string()
        } else {
            format!("The upload could not be read: {}", err.body_text())
        };
        Self { status, kind: "invalid_request_error", message }
    }
}

// ── Auth ──────────────────────────────────────────────────────────────────────

async fn require_key(State(state): State<Arc<AppState>>, request: Request, next: Next) -> Response {
    let Some(expected) = state.api_key.as_deref() else {
        return next.run(request).await;
    };
    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(bearer_token);
    match presented {
        Some(key) if constant_time_eq(key.as_bytes(), expected.as_bytes()) => return next.run(request).await,
        _ => {}
    }
    let error = if presented.is_some() {
        ApiError::unauthorized(
            "The API key was not accepted. Check it against the api_key in the server's server.toml.",
        )
    } else {
        ApiError::unauthorized(
            "This server requires an API key. Send it as 'Authorization: Bearer <key>'; \
             in VoxCtrl, enter it in the Remote speech engine's API key field.",
        )
    };
    // Read the upload before answering. A client is still sending its audio
    // when the key is checked, and a reply sent then closes the connection
    // under it: the client reports "broken pipe" and the person never sees the
    // message above, which is the one that tells them what to fix.
    discard_body(request, state.max_upload_bytes).await;
    error.into_response()
}

/// Read and throw away a request body, up to `limit` bytes.
async fn discard_body(request: Request, limit: usize) {
    let mut stream = request.into_body().into_data_stream();
    let mut seen = 0usize;
    while let Some(Ok(chunk)) = stream.next().await {
        seen = seen.saturating_add(chunk.len());
        if seen > limit {
            break;
        }
    }
}

/// The token from an `Authorization: Bearer <token>` header value.
fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, token) = header.trim().split_once(' ')?;
    let token = token.trim();
    (scheme.eq_ignore_ascii_case("bearer") && !token.is_empty()).then_some(token)
}

/// Compare without stopping at the first differing byte, so how long the
/// comparison takes says nothing about how much of a guessed key was right.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ── Handlers ──────────────────────────────────────────────────────────────────

async fn root() -> Json<serde_json::Value> {
    Json(json!({ "name": "voxctrl-server", "version": env!("CARGO_PKG_VERSION") }))
}

async fn health(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "model": state.engine.info().model_id(),
    }))
}

async fn models(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(json!({
        "object": "list",
        "data": [{ "id": state.engine.info().model_id(), "object": "model", "owned_by": "voxctrl" }],
    }))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum ResponseFormat {
    Json,
    Text,
    VerboseJson,
}

fn parse_response_format(value: &str) -> Result<ResponseFormat, ApiError> {
    match value.trim() {
        "" | "json" => Ok(ResponseFormat::Json),
        "text" => Ok(ResponseFormat::Text),
        "verbose_json" => Ok(ResponseFormat::VerboseJson),
        other => Err(ApiError::bad_request(format!(
            "response_format '{other}' is not supported. Use 'json', 'text' or 'verbose_json'."
        ))),
    }
}

async fn transcriptions(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let mut file: Option<axum::body::Bytes> = None;
    let mut language: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut format = Ok(ResponseFormat::Json);

    // Every field is read before any of them is judged, so a request that is
    // going to be refused has been received in full first (see `require_key`).
    while let Some(field) = multipart.next_field().await? {
        match field.name().unwrap_or_default() {
            "file" => file = Some(field.bytes().await?),
            "language" => language = Some(field.text().await?),
            "prompt" => prompt = Some(field.text().await?),
            "response_format" => format = parse_response_format(&field.text().await?),
            // `model` is accepted and ignored: the server runs the one model it
            // was configured with, whatever name the client has in its settings.
            // Unknown fields (temperature, timestamp_granularities, …) likewise.
            _ => {
                field.bytes().await?;
            }
        }
    }

    let format = format?;
    let file = file.ok_or_else(|| ApiError::bad_request("The request has no 'file' field with the audio to transcribe."))?;
    let samples = audio::decode_wav(&file).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let audio_secs = samples.len() as f64 / audio::TARGET_RATE as f64;

    let engine_state = state.clone();
    let result = tokio::task::spawn_blocking(move || engine_state.engine.transcribe(samples, language, prompt))
        .await
        .map_err(|e| {
            warn!("Transcription task stopped unexpectedly: {e}");
            ApiError::server("The transcription stopped unexpectedly. See the server log.")
        })?
        .map_err(|e| {
            warn!("Transcription failed: {e:#}");
            ApiError::server(format!("Transcription failed: {e:#}"))
        })?;

    // The length of what was said, never the words: this log stays on disk.
    info!(
        "Transcribed {audio_secs:.1}s of audio in {} ms ({} characters)",
        result.inference_ms,
        result.text.chars().count()
    );

    Ok(match format {
        ResponseFormat::Text => result.text.into_response(),
        ResponseFormat::Json => Json(json!({
            "text": result.text,
            "language": result.language,
            "duration": audio_secs,
        }))
        .into_response(),
        ResponseFormat::VerboseJson => Json(json!({
            "task": "transcribe",
            "text": result.text,
            "language": result.language,
            "duration": audio_secs,
            "segments": [],
        }))
        .into_response(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::test_support::fake_engine;
    use axum::body::Body;
    use tower::ServiceExt;

    const KEY: &str = "vx_test_key";
    const BOUNDARY: &str = "----test-boundary";

    fn app(text: &str, fail: bool, key: Option<&str>) -> (Router, Arc<std::sync::Mutex<crate::engine::test_support::Seen>>) {
        let (engine, seen) = fake_engine(text, fail);
        let state = Arc::new(AppState { engine, api_key: key.map(str::to_string), max_upload_bytes: 1024 * 1024 });
        (router(state), seen)
    }

    fn speech_wav() -> Vec<u8> {
        voxctrl_inference::remote_openai::encode_wav(&vec![0.2f32; 16_000])
    }

    /// A multipart body with the given text fields and, optionally, a file.
    fn multipart(fields: &[(&str, &str)], file: Option<&[u8]>) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, value) in fields {
            body.extend_from_slice(
                format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes(),
            );
        }
        if let Some(bytes) = file {
            body.extend_from_slice(
                format!(
                    "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n\
                     Content-Type: audio/wav\r\n\r\n"
                )
                .as_bytes(),
            );
            body.extend_from_slice(bytes);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
        body
    }

    fn post(body: Vec<u8>, key: Option<&str>) -> axum::http::Request<Body> {
        let mut req = axum::http::Request::post("/v1/audio/transcriptions")
            .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={BOUNDARY}"));
        if let Some(key) = key {
            req = req.header(header::AUTHORIZATION, format!("Bearer {key}"));
        }
        req.body(Body::from(body)).unwrap()
    }

    async fn body_json(response: Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn health_needs_no_key() {
        let (app, _) = app("x", false, Some(KEY));
        let res = app.oneshot(axum::http::Request::get("/health").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let json = body_json(res).await;
        assert_eq!(json["status"], "ok");
        assert_eq!(json["model"], "fake/test");
        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn models_lists_the_running_model_in_openai_shape() {
        let (app, _) = app("x", false, Some(KEY));
        let req = axum::http::Request::get("/v1/models")
            .header(header::AUTHORIZATION, format!("Bearer {KEY}"))
            .body(Body::empty())
            .unwrap();
        let json = body_json(app.oneshot(req).await.unwrap()).await;
        assert_eq!(json["object"], "list");
        assert_eq!(json["data"][0]["id"], "fake/test");
    }

    #[tokio::test]
    async fn a_missing_or_wrong_key_is_refused_before_the_model_runs() {
        let (app, seen) = app("x", false, Some(KEY));
        let body = multipart(&[], Some(&speech_wav()));

        let res = app.clone().oneshot(post(body.clone(), None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        let json = body_json(res).await;
        assert_eq!(json["error"]["type"], "authentication_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("requires an API key"));

        let res = app.clone().oneshot(post(body.clone(), Some("vx_wrong_key"))).await.unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

        let res = app
            .oneshot(axum::http::Request::get("/v1/models").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(seen.lock().unwrap().calls, 0);
    }

    #[tokio::test]
    async fn with_no_key_configured_requests_are_accepted() {
        let (app, _) = app("open door", false, None);
        let res = app.oneshot(post(multipart(&[], Some(&speech_wav())), None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["text"], "open door");
    }

    #[tokio::test]
    async fn transcribes_and_passes_language_and_prompt_through() {
        let (app, seen) = app("hello world", false, Some(KEY));
        let body = multipart(
            &[("model", "whisper-1"), ("response_format", "json"), ("language", "en"), ("prompt", "Hey Vox")],
            Some(&speech_wav()),
        );
        let res = app.oneshot(post(body, Some(KEY))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let json = body_json(res).await;
        assert_eq!(json["text"], "hello world");
        assert_eq!(json["language"], "en");
        assert_eq!(json["duration"], 1.0);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.samples, 16_000);
        assert_eq!(seen.language.as_deref(), Some("en"));
        assert_eq!(seen.prompt.as_deref(), Some("Hey Vox"));
    }

    #[tokio::test]
    async fn the_response_parses_with_the_clients_own_parser() {
        // The contract with the desktop app's Remote OpenAI engine.
        let (app, _) = app("round trip", false, Some(KEY));
        let res = app.oneshot(post(multipart(&[], Some(&speech_wav())), Some(KEY))).await.unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        let parsed = voxctrl_inference::remote_openai::parse_transcription_response(&bytes, 1000, 5).unwrap();
        assert_eq!(parsed.text, "round trip");
    }

    #[tokio::test]
    async fn accepts_the_clients_streamed_upload_with_unknown_length() {
        let (app, seen) = app("streamed", false, Some(KEY));
        let mut wav = voxctrl_inference::remote_openai::create_streaming_wav_header().to_vec();
        wav.extend_from_slice(&voxctrl_inference::remote_openai::samples_to_pcm16(&vec![0.2f32; 8_000]));
        let res = app.oneshot(post(multipart(&[("model", "whisper-1")], Some(&wav)), Some(KEY))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(seen.lock().unwrap().samples, 8_000);
    }

    #[tokio::test]
    async fn text_and_verbose_json_formats() {
        let (app, _) = app("plain words", false, Some(KEY));
        let res = app
            .clone()
            .oneshot(post(multipart(&[("response_format", "text")], Some(&speech_wav())), Some(KEY)))
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
        assert_eq!(&bytes[..], b"plain words");

        let res = app
            .oneshot(post(multipart(&[("response_format", "verbose_json")], Some(&speech_wav())), Some(KEY)))
            .await
            .unwrap();
        let json = body_json(res).await;
        assert_eq!(json["task"], "transcribe");
        assert_eq!(json["text"], "plain words");
    }

    #[tokio::test]
    async fn the_silent_connection_test_gets_an_empty_transcript() {
        // What "Test connection" in the app sends: 0.2 s of zeros.
        let (app, seen) = app("should not appear", false, Some(KEY));
        let wav = voxctrl_inference::remote_openai::encode_wav(&vec![0.0f32; 3200]);
        let res = app.oneshot(post(multipart(&[], Some(&wav)), Some(KEY))).await.unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(body_json(res).await["text"], "");
        assert_eq!(seen.lock().unwrap().calls, 0);
    }

    #[tokio::test]
    async fn bad_requests_explain_themselves() {
        let (app, _) = app("x", false, Some(KEY));

        let res = app.clone().oneshot(post(multipart(&[("model", "m")], None), Some(KEY))).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(body_json(res).await["error"]["message"].as_str().unwrap().contains("'file'"));

        let res = app.clone().oneshot(post(multipart(&[], Some(b"ID3 this is an mp3")), Some(KEY))).await.unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(body_json(res).await["error"]["message"].as_str().unwrap().contains("not WAV"));

        let res = app
            .oneshot(post(multipart(&[("response_format", "srt")], Some(&speech_wav())), Some(KEY)))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert!(body_json(res).await["error"]["message"].as_str().unwrap().contains("srt"));
    }

    #[tokio::test]
    async fn a_model_failure_is_a_500_in_the_error_envelope() {
        let (app, _) = app("x", true, Some(KEY));
        let res = app.oneshot(post(multipart(&[], Some(&speech_wav())), Some(KEY))).await.unwrap();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let json = body_json(res).await;
        assert_eq!(json["error"]["type"], "server_error");
        assert!(json["error"]["message"].as_str().unwrap().contains("fell over"));
    }

    #[tokio::test]
    async fn an_upload_over_the_limit_is_a_413_that_names_the_setting() {
        let (engine, _) = fake_engine("x", false);
        let state = Arc::new(AppState { engine, api_key: None, max_upload_bytes: 1024 });
        let app = router(state);
        let res = app.oneshot(post(multipart(&[], Some(&speech_wav())), None)).await.unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(body_json(res).await["error"]["message"].as_str().unwrap().contains("max_upload_mb"));
    }

    #[test]
    fn bearer_token_parsing() {
        assert_eq!(bearer_token("Bearer abc"), Some("abc"));
        assert_eq!(bearer_token("bearer   abc  "), Some("abc"));
        assert_eq!(bearer_token("Basic abc"), None);
        assert_eq!(bearer_token("Bearer "), None);
        assert_eq!(bearer_token("abc"), None);
    }

    #[test]
    fn constant_time_eq_compares_whole_values() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"sama"));
        assert!(!constant_time_eq(b"same", b"same-but-longer"));
        assert!(constant_time_eq(b"", b""));
    }
}
