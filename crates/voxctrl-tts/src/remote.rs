//! The client side of text-to-speech on a VoxCtrl server: send the text, get a
//! WAV back to play here.
//!
//! The endpoint (`/v1/audio/speech`) has the shape of OpenAI's, but which
//! engine and voice speak is the server's setting, not the request's.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

/// Where the server is and the key it wants: the Remote Speech Engine's
/// address and key, since it is the same server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLink {
    pub endpoint: String,
    pub api_key: Option<String>,
}

/// A server that is not there should be found out quickly.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
/// Synthesis of a long passage on a slow machine can take a while; past this
/// the speech is no longer worth waiting for.
const SPEECH_TIMEOUT: Duration = Duration::from_secs(120);

/// The `/v1/audio/speech` URL for a speech endpoint, however it was entered.
pub fn speech_url(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if let Some(base) = trimmed.strip_suffix("/audio/transcriptions") {
        format!("{base}/audio/speech")
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/audio/speech")
    } else {
        format!("{trimmed}/v1/audio/speech")
    }
}

fn with_key(req: reqwest::blocking::RequestBuilder, api_key: Option<&str>) -> reqwest::blocking::RequestBuilder {
    match api_key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(key) => req.bearer_auth(key),
        None => req,
    }
}

fn error_message(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(value.get("error")?.get("message")?.as_str()?.to_string())
}

fn client(timeout: Duration) -> Result<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(timeout)
        .build()
        .context("Could not set up the connection")
}

/// Have the server speak `text` and return the WAV. Blocking: call it from a
/// thread that is not an async runtime's. Errors are worded for the user.
pub fn fetch_speech(link: &ServerLink, text: &str) -> Result<Vec<u8>> {
    if link.endpoint.trim().is_empty() {
        return Err(anyhow!(
            "No server address is set. Enter it under Settings → Engine → Remote Speech Engine Settings."
        ));
    }
    let request = with_key(client(SPEECH_TIMEOUT)?.post(speech_url(&link.endpoint)), link.api_key.as_deref())
        .json(&serde_json::json!({ "input": text, "response_format": "wav" }));
    let response = request.send().map_err(|e| {
        if e.is_connect() {
            anyhow!("Could not reach the server to speak. Check that it is running and the address is right.")
        } else if e.is_timeout() {
            anyhow!("The server took too long to speak.")
        } else {
            anyhow!("The request to the server failed: {e}")
        }
    })?;
    let status = response.status();
    let body = response.bytes().context("The server's audio could not be read")?;
    if !status.is_success() {
        return Err(match (status.as_u16(), error_message(&body)) {
            (401 | 403, _) => anyhow!("The server rejected the API key (HTTP {}).", status.as_u16()),
            (404 | 405, _) => anyhow!(
                "This server does not offer text-to-speech. It needs voxctrl-server 0.9.7 or newer."
            ),
            (code, Some(message)) => anyhow!("The server could not speak (HTTP {code}): {message}"),
            (code, None) => anyhow!("The server could not speak (HTTP {code})."),
        });
    }
    if body.len() < 44 || &body[0..4] != b"RIFF" {
        return Err(anyhow!("The server's reply was not audio."));
    }
    Ok(body.to_vec())
}

/// Whether a server offers text-to-speech, in words for the Settings window.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerTtsStatus {
    pub available: bool,
    /// The engine and voice the server speaks with, when it says.
    pub engine: Option<String>,
    pub voice: Option<String>,
    /// What to tell the user, including what to do when it is not available.
    pub message: String,
}

impl ServerTtsStatus {
    fn unavailable(message: impl Into<String>) -> Self {
        Self { available: false, engine: None, voice: None, message: message.into() }
    }
}

/// Ask the server whether it speaks. Never an error: every way this can go
/// wrong is something to show the user. Blocking.
pub fn check_server_tts(link: &ServerLink) -> ServerTtsStatus {
    if link.endpoint.trim().is_empty() {
        return ServerTtsStatus::unavailable(
            "No server address is set. Enter it under Engine → Remote Speech Engine Settings.",
        );
    }
    let client = match client(Duration::from_secs(6)) {
        Ok(c) => c,
        Err(e) => return ServerTtsStatus::unavailable(format!("{e:#}")),
    };
    let response = match with_key(client.get(speech_url(&link.endpoint)), link.api_key.as_deref()).send() {
        Ok(r) => r,
        Err(e) if e.is_connect() => return ServerTtsStatus::unavailable("Could not connect to the server."),
        Err(e) if e.is_timeout() => return ServerTtsStatus::unavailable("The server did not answer in time."),
        Err(_) => return ServerTtsStatus::unavailable("The request to the server failed."),
    };
    let status = response.status().as_u16();
    let body = response.bytes().unwrap_or_default();
    interpret_status(status, &body)
}

fn interpret_status(status: u16, body: &[u8]) -> ServerTtsStatus {
    match status {
        200 => {
            let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
            let (engine, voice) = (text("engine"), text("voice"));
            match value.get("available").and_then(|a| a.as_bool()) {
                Some(true) => ServerTtsStatus {
                    available: true,
                    message: match (&engine, &voice) {
                        (Some(engine), Some(voice)) => format!("This server speaks with {engine}, voice {voice}."),
                        (Some(engine), None) => format!("This server speaks with {engine}."),
                        _ => "This server offers text-to-speech.".into(),
                    },
                    engine,
                    voice,
                },
                Some(false) => ServerTtsStatus::unavailable(
                    "Text-to-speech is turned off on this server. On the server, set enabled = true under [tts] \
                     in server.toml (or VOXCTRL_SERVER_TTS=true) and restart it.",
                ),
                None => ServerTtsStatus::unavailable("This server does not offer text-to-speech."),
            }
        }
        401 | 403 => ServerTtsStatus::unavailable(format!("The server rejected the API key (HTTP {status}).")),
        404 | 405 => ServerTtsStatus::unavailable(
            "This server does not offer text-to-speech. It needs voxctrl-server 0.9.7 or newer; \
             other speech servers do not have this check.",
        ),
        _ => ServerTtsStatus::unavailable(format!("The server answered with an error (HTTP {status}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_url_follows_the_endpoint_however_it_was_entered() {
        for endpoint in
            ["http://box:8787/v1", "http://box:8787/v1/", "http://box:8787", " http://box:8787/v1/audio/transcriptions "]
        {
            assert_eq!(speech_url(endpoint), "http://box:8787/v1/audio/speech", "{endpoint}");
        }
    }

    #[test]
    fn status_is_read_from_what_the_server_says() {
        let on = interpret_status(200, br#"{"available": true, "engine": "piper", "voice": "en-us-amy-low"}"#);
        assert!(on.available);
        assert_eq!(on.message, "This server speaks with piper, voice en-us-amy-low.");

        let off = interpret_status(200, br#"{"available": false}"#);
        assert!(!off.available);
        assert!(off.message.contains("VOXCTRL_SERVER_TTS"), "{}", off.message);

        assert!(!interpret_status(200, b"<html>").available);
        assert!(interpret_status(404, b"").message.contains("voxctrl-server"));
        assert!(interpret_status(401, b"").message.contains("API key"));
    }

    #[test]
    fn no_address_is_explained_without_a_request() {
        let link = ServerLink { endpoint: "  ".into(), api_key: None };
        assert!(fetch_speech(&link, "hi").unwrap_err().to_string().contains("No server address"));
        assert!(check_server_tts(&link).message.contains("No server address"));
    }

    #[test]
    fn an_unreachable_server_is_said_plainly() {
        let link = ServerLink { endpoint: "http://127.0.0.1:1/v1".into(), api_key: None };
        assert!(fetch_speech(&link, "hi").unwrap_err().to_string().contains("Could not reach the server"));
        assert_eq!(check_server_tts(&link).message, "Could not connect to the server.");
    }
}
