use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use reqwest::header::CONTENT_TYPE;
use tracing::{debug, warn};
use voxctrl_config::RemoteOpenAiConfig;

use crate::backend::{TranscribeRequest, TranscriptionBackend, TranscriptionResult};

/// Normalizes an endpoint into the full `/v1/audio/transcriptions` URL.
pub fn normalize_transcription_url(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/audio/transcriptions") {
        trimmed.to_string()
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/audio/transcriptions")
    } else {
        format!("{trimmed}/v1/audio/transcriptions")
    }
}

/// Normalizes an endpoint into the `/v1/models` URL for probing available models.
pub fn normalize_models_url(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/audio/transcriptions") {
        let base = &trimmed[..trimmed.len() - "/audio/transcriptions".len()];
        format!("{base}/models")
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/models")
    } else {
        format!("{trimmed}/v1/models")
    }
}

/// Convert 16 kHz mono f32 samples to 16-bit PCM little-endian bytes.
pub fn samples_to_pcm16(samples: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(samples.len() * 2);
    for &sample in samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let s = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&s.to_le_bytes());
    }
    bytes
}

/// Create a 44-byte WAV header for streaming (unknown/infinite data size).
pub fn create_streaming_wav_header() -> [u8; 44] {
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    header[22..24].copy_from_slice(&1u16.to_le_bytes()); // Mono
    header[24..28].copy_from_slice(&16000u32.to_le_bytes()); // 16 kHz
    header[28..32].copy_from_slice(&(16000u32 * 2).to_le_bytes()); // Byte rate
    header[32..34].copy_from_slice(&2u16.to_le_bytes()); // Block align
    header[34..36].copy_from_slice(&16u16.to_le_bytes()); // 16 bits
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&0xFFFF_FFFFu32.to_le_bytes());
    header
}

/// Create a standard 44-byte WAV header for known PCM byte length.
pub fn create_wav_header(pcm_byte_len: u32) -> [u8; 44] {
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(pcm_byte_len + 36).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    header[22..24].copy_from_slice(&1u16.to_le_bytes()); // Mono
    header[24..28].copy_from_slice(&16000u32.to_le_bytes()); // 16 kHz
    header[28..32].copy_from_slice(&(16000u32 * 2).to_le_bytes());
    header[32..34].copy_from_slice(&2u16.to_le_bytes());
    header[34..36].copy_from_slice(&16u16.to_le_bytes());
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&pcm_byte_len.to_le_bytes());
    header
}

/// Encode f32 samples at 16 kHz mono into a complete WAV file.
pub fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let pcm = samples_to_pcm16(samples);
    let header = create_wav_header(pcm.len() as u32);
    let mut out = Vec::with_capacity(header.len() + pcm.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&pcm);
    out
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteSttTestResult {
    pub success: bool,
    pub message: String,
    pub models: Vec<String>,
}

/// Parse OpenAI `/v1/audio/transcriptions` response body.
pub fn parse_transcription_response(
    bytes: &[u8],
    duration_ms: u32,
    inference_ms: u32,
) -> Result<TranscriptionResult> {
    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(bytes) {
        if let Some(err_obj) = val.get("error") {
            let msg = err_obj
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown remote error");
            return Err(anyhow!("Remote speech engine error: {msg}"));
        }

        if let Some(text) = val.get("text").and_then(|t| t.as_str()) {
            let language = val
                .get("language")
                .and_then(|l| l.as_str())
                .unwrap_or("en")
                .to_string();
            return Ok(TranscriptionResult {
                text: text.to_string(),
                language,
                language_probability: 1.0,
                duration_ms,
                inference_ms,
                word_timestamps: None,
            });
        }
    }

    // Some servers return plain text
    let text = String::from_utf8_lossy(bytes).trim().to_string();
    if !text.is_empty() {
        return Ok(TranscriptionResult {
            text,
            language: "en".into(),
            language_probability: 1.0,
            duration_ms,
            inference_ms,
            word_timestamps: None,
        });
    }

    Err(anyhow!(
        "Unexpected response format from remote speech engine: {}",
        String::from_utf8_lossy(bytes)
    ))
}

fn attach_auth(req: reqwest::RequestBuilder, api_key: Option<&str>) -> reqwest::RequestBuilder {
    match api_key {
        Some(k) if !k.trim().is_empty() => req.bearer_auth(k.trim()),
        _ => req,
    }
}

// ── Streaming Session ─────────────────────────────────────────────────────────

/// An active streaming transcription session.
/// Uploads audio chunks to the remote OpenAI STT endpoint over HTTP chunked streaming
/// while recording is active, minimizing end-of-speech turnaround time.
pub struct RemoteStreamingSession {
    chunk_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<f32>>>,
    result_rx: tokio::sync::oneshot::Receiver<Result<TranscriptionResult>>,
    buffered_samples: Arc<Mutex<Vec<f32>>>,
}

impl RemoteStreamingSession {
    /// Start a new streaming session. Connects to the remote endpoint and starts streaming
    /// multipart body immediately.
    pub fn start(
        config: RemoteOpenAiConfig,
        initial_prompt: Option<String>,
        handle: &tokio::runtime::Handle,
    ) -> Self {
        let (chunk_tx, mut chunk_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<f32>>();
        let (result_tx, result_rx) = tokio::sync::oneshot::channel();

        let buffered_samples = Arc::new(Mutex::new(Vec::<f32>::new()));
        let buffered_for_stream = buffered_samples.clone();
        let buffered_for_fallback = buffered_samples.clone();

        let cfg_clone = config.clone();
        let prompt_clone = initial_prompt.clone();

        handle.spawn(async move {
            let start_inst = Instant::now();
            let boundary = format!("----VoxCtrlBoundary{:016x}", rand_u64());

            // 1. Build multipart preamble
            let mut preamble = Vec::new();

            // model field
            preamble.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            preamble.extend_from_slice(b"Content-Disposition: form-data; name=\"model\"\r\n\r\n");
            preamble.extend_from_slice(cfg_clone.model.as_bytes());
            preamble.extend_from_slice(b"\r\n");

            // response_format field
            preamble.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            preamble.extend_from_slice(b"Content-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n");

            // language field (if set)
            if !cfg_clone.language.trim().is_empty() && cfg_clone.language.trim() != "auto" {
                preamble.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                preamble.extend_from_slice(b"Content-Disposition: form-data; name=\"language\"\r\n\r\n");
                preamble.extend_from_slice(cfg_clone.language.trim().as_bytes());
                preamble.extend_from_slice(b"\r\n");
            }

            // prompt field (if provided)
            if let Some(ref p) = prompt_clone {
                if !p.trim().is_empty() {
                    preamble.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
                    preamble.extend_from_slice(b"Content-Disposition: form-data; name=\"prompt\"\r\n\r\n");
                    preamble.extend_from_slice(p.trim().as_bytes());
                    preamble.extend_from_slice(b"\r\n");
                }
            }

            // file header
            preamble.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            preamble.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\n");
            preamble.extend_from_slice(b"Content-Type: audio/wav\r\n\r\n");

            // 44-byte streaming WAV header
            let wav_header = create_streaming_wav_header();
            preamble.extend_from_slice(&wav_header);

            let trailing_boundary = format!("\r\n--{boundary}--\r\n");

            // Stream construction
            let initial_bytes = Some(Bytes::from(preamble));
            let trailing_bytes = Some(Bytes::from(trailing_boundary));

            enum StreamState {
                Preamble(Bytes),
                Streaming,
                Trailing(Bytes),
                Done,
            }

            let mut state = StreamState::Preamble(initial_bytes.unwrap());

            let body_stream = futures_util::stream::poll_fn(move |_cx| {
                match &mut state {
                    StreamState::Preamble(bytes) => {
                        let b = std::mem::take(bytes);
                        state = StreamState::Streaming;
                        std::task::Poll::Ready(Some(Ok::<Bytes, std::io::Error>(b)))
                    }
                    StreamState::Streaming => {
                        match chunk_rx.poll_recv(_cx) {
                            std::task::Poll::Ready(Some(chunk)) => {
                                {
                                    let mut buf = buffered_for_stream.lock().unwrap();
                                    buf.extend_from_slice(&chunk);
                                }
                                let pcm = samples_to_pcm16(&chunk);
                                std::task::Poll::Ready(Some(Ok(Bytes::from(pcm))))
                            }
                            std::task::Poll::Ready(None) => {
                                // Channel closed -> emit trailing boundary
                                if let Some(trailing) = trailing_bytes.clone() {
                                    state = StreamState::Trailing(trailing);
                                    if let StreamState::Trailing(b) = &mut state {
                                        let taken = std::mem::take(b);
                                        state = StreamState::Done;
                                        std::task::Poll::Ready(Some(Ok(taken)))
                                    } else {
                                        std::task::Poll::Ready(None)
                                    }
                                } else {
                                    state = StreamState::Done;
                                    std::task::Poll::Ready(None)
                                }
                            }
                            std::task::Poll::Pending => std::task::Poll::Pending,
                        }
                    }
                    StreamState::Trailing(bytes) => {
                        let b = std::mem::take(bytes);
                        state = StreamState::Done;
                        std::task::Poll::Ready(Some(Ok(b)))
                    }
                    StreamState::Done => std::task::Poll::Ready(None),
                }
            });

            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(cfg_clone.timeout_secs))
                .build();

            let client = match client {
                Ok(c) => c,
                Err(e) => {
                    let _ = result_tx.send(Err(anyhow!("Failed to build HTTP client: {e}")));
                    return;
                }
            };

            let url = normalize_transcription_url(&cfg_clone.endpoint);
            let mut req = client
                .post(&url)
                .header(
                    CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(reqwest::Body::wrap_stream(body_stream));

            req = attach_auth(req, cfg_clone.api_key.as_deref());

            debug!("Starting streaming upload to {url}");
            let res = req.send().await;

            match res {
                Ok(resp) if resp.status().is_success() => {
                    let inference_ms = start_inst.elapsed().as_millis() as u32;
                    let body_bytes = resp.bytes().await.unwrap_or_default();
                    let total_samples = buffered_for_fallback.lock().unwrap().len();
                    let duration_ms = (total_samples as f32 / 16.0) as u32;
                    let result = parse_transcription_response(&body_bytes, duration_ms, inference_ms);
                    let _ = result_tx.send(result);
                }
                err => {
                    // Fallback path: If the streaming upload failed (e.g. server doesn't support
                    // chunked multipart or HTTP 411 Length Required), retry with standard
                    // buffered multipart upload.
                    warn!(
                        "Streaming upload failed or rejected ({:?}), retrying with standard multipart",
                        err.as_ref().map(|r| r.status())
                    );

                    let samples = buffered_for_fallback.lock().unwrap().clone();
                    if samples.is_empty() {
                        let _ = result_tx.send(Err(anyhow!("No audio was captured")));
                        return;
                    }

                    let fallback_res = execute_multipart_transcribe(
                        &client,
                        &cfg_clone,
                        prompt_clone.as_deref(),
                        &samples,
                        start_inst,
                    )
                    .await;
                    let _ = result_tx.send(fallback_res);
                }
            }
        });

        Self {
            chunk_tx: Some(chunk_tx),
            result_rx,
            buffered_samples,
        }
    }

    /// Feed a newly recorded audio chunk to the live stream.
    pub fn send_chunk(&self, chunk: Vec<f32>) {
        if let Some(ref tx) = self.chunk_tx {
            let _ = tx.send(chunk);
        }
    }

    /// Complete recording, signal EOF on the upload stream, and await the transcription response.
    pub async fn finish(mut self) -> Result<TranscriptionResult> {
        // Drop the channel sender to signal end of stream
        drop(self.chunk_tx.take());

        self.result_rx
            .await
            .map_err(|_| anyhow!("Streaming transcription task cancelled"))?
    }

    /// Blocking finish for synchronous execution in coordinator threads.
    pub fn finish_sync(self, handle: &tokio::runtime::Handle) -> Result<TranscriptionResult> {
        handle.block_on(self.finish())
    }

    /// Access the accumulated samples for noise-gate / RMS checks.
    pub fn buffered_samples(&self) -> Vec<f32> {
        self.buffered_samples.lock().unwrap().clone()
    }
}

/// Fallback / Standard multipart POST to `/v1/audio/transcriptions`.
async fn execute_multipart_transcribe(
    client: &reqwest::Client,
    cfg: &RemoteOpenAiConfig,
    prompt: Option<&str>,
    audio: &[f32],
    start_inst: Instant,
) -> Result<TranscriptionResult> {
    let url = normalize_transcription_url(&cfg.endpoint);
    let wav_bytes = encode_wav(audio);
    let duration_ms = (audio.len() as f32 / 16.0) as u32;

    let mut form = reqwest::multipart::Form::new()
        .text("model", cfg.model.clone())
        .text("response_format", "json")
        .part(
            "file",
            reqwest::multipart::Part::bytes(wav_bytes)
                .file_name("audio.wav")
                .mime_str("audio/wav")?,
        );

    if !cfg.language.trim().is_empty() && cfg.language.trim() != "auto" {
        form = form.text("language", cfg.language.trim().to_string());
    }

    if let Some(p) = prompt {
        if !p.trim().is_empty() {
            form = form.text("prompt", p.trim().to_string());
        }
    }

    let mut req = client.post(&url).multipart(form);
    req = attach_auth(req, cfg.api_key.as_deref());

    let resp = req
        .send()
        .await
        .with_context(|| format!("Failed to send transcription request to {url}"))?;

    let inference_ms = start_inst.elapsed().as_millis() as u32;
    let status = resp.status();
    let body_bytes = resp.bytes().await.unwrap_or_default();

    if !status.is_success() {
        let err_text = String::from_utf8_lossy(&body_bytes);
        return Err(anyhow!(
            "Remote speech engine returned HTTP {status}: {err_text}"
        ));
    }

    parse_transcription_response(&body_bytes, duration_ms, inference_ms)
}

// ── Backend Implementation ───────────────────────────────────────────────────

/// TranscriptionBackend implementation for remote OpenAI-compatible STT.
pub struct RemoteOpenAiBackend {
    config: RemoteOpenAiConfig,
    client: reqwest::Client,
}

impl RemoteOpenAiBackend {
    pub fn new(config: RemoteOpenAiConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self { config, client }
    }
}

impl TranscriptionBackend for RemoteOpenAiBackend {
    fn name(&self) -> &str {
        "remote-openai"
    }

    fn load(&mut self) -> Result<()> {
        Ok(())
    }

    fn transcribe(&self, req: &TranscribeRequest) -> Result<TranscriptionResult> {
        let start = Instant::now();
        let client = self.client.clone();
        let cfg = self.config.clone();
        let prompt = req.initial_prompt.clone();
        let audio = req.audio.clone();

        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                std::thread::spawn(move || {
                    handle.block_on(async move {
                        execute_multipart_transcribe(&client, &cfg, prompt.as_deref(), &audio, start).await
                    })
                })
                .join()
                .unwrap_or_else(|_| Err(anyhow!("Transcription thread panicked")))
            }
            Err(_) => {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()?;
                rt.block_on(async move {
                    execute_multipart_transcribe(&client, &cfg, prompt.as_deref(), &audio, start).await
                })
            }
        }
    }

    fn unload(&mut self) {}

    fn is_loaded(&self) -> bool {
        true
    }
}

// ── Connection Testing ────────────────────────────────────────────────────────

/// Probes a remote speech engine by sending a minimal test audio chunk to `/v1/audio/transcriptions`
/// and querying `/v1/models` for available models.
pub async fn test_remote_speech_engine(
    endpoint: &str,
    api_key: Option<&str>,
    model: &str,
    timeout_secs: u64,
) -> Result<RemoteSttTestResult> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .context("Failed to build HTTP client")?;

    let trans_url = normalize_transcription_url(endpoint);

    // 1. Probe `/v1/audio/transcriptions` with a tiny 0.2s silence WAV (3200 samples)
    let silence = vec![0.0f32; 3200];
    let wav_bytes = encode_wav(&silence);

    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "json")
        .part(
            "file",
            reqwest::multipart::Part::bytes(wav_bytes)
                .file_name("test_audio.wav")
                .mime_str("audio/wav")?,
        );

    let mut trans_req = client.post(&trans_url).multipart(form);
    trans_req = attach_auth(trans_req, api_key);

    let trans_res = trans_req.send().await;

    // 2. Concurrently or additionally probe `/v1/models` to discover model choices
    let models_url = normalize_models_url(endpoint);
    let mut models_req = client.get(&models_url);
    models_req = attach_auth(models_req, api_key);
    let mut discovered_models = Vec::new();

    if let Ok(models_resp) = models_req.send().await {
        if models_resp.status().is_success() {
            if let Ok(json) = models_resp.json::<serde_json::Value>().await {
                if let Some(data) = json.get("data").and_then(|d| d.as_array()) {
                    for item in data {
                        if let Some(id) = item.get("id").and_then(|i| i.as_str()) {
                            discovered_models.push(id.to_string());
                        }
                    }
                }
            }
        }
    }

    match trans_res {
        Ok(resp) => {
            let status = resp.status();
            if status.is_success() {
                Ok(RemoteSttTestResult {
                    success: true,
                    message: "Successfully connected to remote speech engine!".to_string(),
                    models: discovered_models,
                })
            } else {
                let err_body = resp.text().await.unwrap_or_default();
                let detail = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&err_body) {
                    val.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .map(str::to_string)
                        .unwrap_or(err_body)
                } else {
                    err_body
                };
                Ok(RemoteSttTestResult {
                    success: false,
                    message: format!("Server returned HTTP {status}: {detail}"),
                    models: discovered_models,
                })
            }
        }
        Err(e) => Ok(RemoteSttTestResult {
            success: false,
            message: format!("Connection failed: {e}"),
            models: discovered_models,
        }),
    }
}

fn rand_u64() -> u64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    now.as_nanos() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_transcription_url() {
        assert_eq!(
            normalize_transcription_url("http://localhost:8000"),
            "http://localhost:8000/v1/audio/transcriptions"
        );
        assert_eq!(
            normalize_transcription_url("http://localhost:8000/"),
            "http://localhost:8000/v1/audio/transcriptions"
        );
        assert_eq!(
            normalize_transcription_url("http://localhost:8000/v1"),
            "http://localhost:8000/v1/audio/transcriptions"
        );
        assert_eq!(
            normalize_transcription_url("http://localhost:8000/v1/audio/transcriptions"),
            "http://localhost:8000/v1/audio/transcriptions"
        );
        assert_eq!(
            normalize_transcription_url("http://localhost:8000/v1/audio/transcriptions/"),
            "http://localhost:8000/v1/audio/transcriptions"
        );
    }

    #[test]
    fn test_normalize_models_url() {
        assert_eq!(
            normalize_models_url("http://localhost:8000"),
            "http://localhost:8000/v1/models"
        );
        assert_eq!(
            normalize_models_url("http://localhost:8000/v1"),
            "http://localhost:8000/v1/models"
        );
        assert_eq!(
            normalize_models_url("http://localhost:8000/v1/audio/transcriptions"),
            "http://localhost:8000/v1/models"
        );
    }

    #[test]
    fn test_wav_encoding() {
        let samples = vec![0.0f32; 1600]; // 100ms of silence
        let wav = encode_wav(&samples);
        assert_eq!(wav.len(), 44 + 3200);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
    }

    #[test]
    fn test_parse_transcription_response() {
        let json_resp = br#"{"text": "Hello world", "language": "en"}"#;
        let res = parse_transcription_response(json_resp, 1000, 200).unwrap();
        assert_eq!(res.text, "Hello world");
        assert_eq!(res.language, "en");

        let err_resp = br#"{"error": {"message": "Invalid model"}}"#;
        assert!(parse_transcription_response(err_resp, 1000, 200).is_err());
    }
}
