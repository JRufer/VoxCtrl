//! Dictation cleanup on a VoxCtrl server, for a client that has handed its
//! transcription to one and wants cleanup done there too.
//!
//! This is VoxCtrl's own endpoint (`/v1/cleanup`), not part of the OpenAI API:
//! only `voxctrl-server` answers it. Any other speech server returns 404,
//! which [`check_server_cleanup`] reports as "does not offer it".

use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use voxctrl_config::RemoteOpenAiConfig;

/// How long to wait to connect. Matches the transcription client: a server
/// that is not there should be found out quickly.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);

/// The `/v1/cleanup` URL for a speech endpoint, however it was entered.
pub fn cleanup_url(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if let Some(base) = trimmed.strip_suffix("/audio/transcriptions") {
        format!("{base}/cleanup")
    } else if trimmed.ends_with("/v1") {
        format!("{trimmed}/cleanup")
    } else {
        format!("{trimmed}/v1/cleanup")
    }
}

/// How long one cleanup may take: the transcription timeout the user set,
/// held between 5 and 30 seconds. Cleanup sits between releasing the key and
/// the text appearing, so past that the raw transcript is the better outcome.
fn request_timeout(cfg: &RemoteOpenAiConfig) -> Duration {
    Duration::from_secs(cfg.timeout_secs.clamp(5, 30))
}

fn client(endpoint: &str, tls_fingerprint: Option<&str>, timeout: Duration) -> Result<reqwest::Client> {
    let builder = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT).timeout(timeout);
    voxctrl_pairing::pin(builder, endpoint, tls_fingerprint)?.build().context("Could not set up the connection")
}

fn with_key(req: reqwest::RequestBuilder, api_key: Option<&str>) -> reqwest::RequestBuilder {
    match api_key.map(str::trim).filter(|k| !k.is_empty()) {
        Some(key) => req.bearer_auth(key),
        None => req,
    }
}

/// The server's own explanation from an error reply, when it sent one in the
/// OpenAI envelope.
fn error_message(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    Some(value.get("error")?.get("message")?.as_str()?.to_string())
}

/// Clean `text` on the server in the given style.
///
/// An error means the caller should use the text as it was: the server is
/// unreachable, has cleanup turned off, or is not a VoxCtrl server.
pub async fn clean_on_server(cfg: &RemoteOpenAiConfig, text: &str, styling: &str) -> Result<String> {
    let request = with_key(
        client(&cfg.endpoint, cfg.tls_fingerprint.as_deref(), request_timeout(cfg))?.post(cleanup_url(&cfg.endpoint)),
        cfg.api_key.as_deref(),
    )
    .json(&serde_json::json!({ "text": text, "styling": styling }));
    let response = request.send().await.context("The cleanup request did not reach the server")?;
    let status = response.status();
    let body = response.bytes().await.context("The server's cleanup reply could not be read")?;
    if !status.is_success() {
        return Err(match error_message(&body) {
            Some(message) => anyhow!("The server did not clean up the text (HTTP {}): {message}", status.as_u16()),
            None => anyhow!("The server did not clean up the text (HTTP {})", status.as_u16()),
        });
    }
    let value: serde_json::Value = serde_json::from_slice(&body).context("The server's cleanup reply was not JSON")?;
    value
        .get("text")
        .and_then(|t| t.as_str())
        .map(str::to_string)
        .ok_or_else(|| anyhow!("The server's cleanup reply had no text"))
}

/// Whether a server offers cleanup, in words for the Settings window.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ServerCleanupStatus {
    pub available: bool,
    /// The cleanup model the server runs, when it says.
    pub model: Option<String>,
    /// What to tell the user, including what to do when it is not available.
    pub message: String,
}

impl ServerCleanupStatus {
    fn unavailable(message: impl Into<String>) -> Self {
        Self { available: false, model: None, message: message.into() }
    }
}

/// Ask the server whether it cleans up dictation. Never an error: every way
/// this can go wrong is something to show the user.
pub async fn check_server_cleanup(
    endpoint: &str,
    api_key: Option<&str>,
    tls_fingerprint: Option<&str>,
) -> ServerCleanupStatus {
    let client = match client(endpoint, tls_fingerprint, Duration::from_secs(6)) {
        Ok(c) => c,
        Err(e) => return ServerCleanupStatus::unavailable(format!("{e:#}")),
    };
    let response = match with_key(client.get(cleanup_url(endpoint)), api_key).send().await {
        Ok(r) => r,
        Err(e) if voxctrl_pairing::is_certificate_mismatch(&e) => {
            return ServerCleanupStatus::unavailable(voxctrl_pairing::CERTIFICATE_MISMATCH_HELP)
        }
        Err(e) if e.is_connect() => return ServerCleanupStatus::unavailable("Could not connect to the server."),
        Err(e) if e.is_timeout() => return ServerCleanupStatus::unavailable("The server did not answer in time."),
        Err(_) => return ServerCleanupStatus::unavailable("The request to the server failed."),
    };
    let status = response.status().as_u16();
    let body = response.bytes().await.unwrap_or_default();
    interpret_status(status, &body)
}

fn interpret_status(status: u16, body: &[u8]) -> ServerCleanupStatus {
    match status {
        200 => {
            let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let model = value.get("model").and_then(|m| m.as_str()).map(str::to_string);
            match value.get("available").and_then(|a| a.as_bool()) {
                Some(true) => ServerCleanupStatus {
                    available: true,
                    message: match &model {
                        Some(model) => format!("This server cleans up dictation ({model})."),
                        None => "This server cleans up dictation.".into(),
                    },
                    model,
                },
                Some(false) => ServerCleanupStatus::unavailable(
                    "Cleanup is turned off on this server. On the server, set enabled = true under [cleanup] \
                     in server.toml (or VOXCTRL_SERVER_CLEANUP=true) and restart it.",
                ),
                None => ServerCleanupStatus::unavailable("This server does not offer dictation cleanup."),
            }
        }
        401 | 403 => ServerCleanupStatus::unavailable(format!("The server rejected the API key (HTTP {status}).")),
        404 | 405 => ServerCleanupStatus::unavailable(
            "This server does not offer dictation cleanup. It needs voxctrl-server 0.9.5 or newer; \
             other speech servers do not have it.",
        ),
        _ => ServerCleanupStatus::unavailable(format!("The server answered with an error (HTTP {status}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_url_follows_the_endpoint_however_it_was_entered() {
        for endpoint in [
            "http://box:8787/v1",
            "http://box:8787/v1/",
            "http://box:8787",
            " http://box:8787/v1/audio/transcriptions ",
        ] {
            assert_eq!(cleanup_url(endpoint), "http://box:8787/v1/cleanup", "{endpoint}");
        }
    }

    #[test]
    fn timeout_follows_the_users_setting_within_bounds() {
        let with = |timeout_secs| RemoteOpenAiConfig { timeout_secs, ..Default::default() };
        assert_eq!(request_timeout(&with(1)), Duration::from_secs(5));
        assert_eq!(request_timeout(&with(12)), Duration::from_secs(12));
        assert_eq!(request_timeout(&with(300)), Duration::from_secs(30));
    }

    #[test]
    fn status_is_read_from_what_the_server_says() {
        let on = interpret_status(200, br#"{"available": true, "model": "s1-mini"}"#);
        assert!(on.available);
        assert_eq!(on.model.as_deref(), Some("s1-mini"));
        assert_eq!(on.message, "This server cleans up dictation (s1-mini).");

        let off = interpret_status(200, br#"{"available": false, "model": null}"#);
        assert!(!off.available);
        assert!(off.message.contains("VOXCTRL_SERVER_CLEANUP"), "{}", off.message);

        // Some other server that happens to answer 200 on this path.
        assert!(!interpret_status(200, b"<html>hello</html>").available);

        let other = interpret_status(404, b"Not Found");
        assert!(!other.available);
        assert!(other.message.contains("voxctrl-server"), "{}", other.message);

        assert!(interpret_status(401, b"").message.contains("API key"));
        assert!(interpret_status(500, b"").message.contains("HTTP 500"));
    }

    #[test]
    fn the_servers_own_explanation_is_passed_on() {
        assert_eq!(
            error_message(br#"{"error": {"message": "Cleanup is not enabled", "type": "not_enabled"}}"#).as_deref(),
            Some("Cleanup is not enabled")
        );
        assert_eq!(error_message(b"Bad Gateway"), None);
    }
}
