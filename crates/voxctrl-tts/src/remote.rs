//! The client side of text-to-speech on a VoxCtrl server: send the text, get a
//! WAV back to play here.
//!
//! The endpoint (`/v1/audio/speech`) has the shape of OpenAI's, but which
//! engine and voice speak is the server's setting, not the request's.

use std::time::Duration;

use anyhow::{anyhow, Context, Result};

/// Where the server is and the key it wants: the Remote Speech Engine's
/// address and key, since it is the same server.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServerLink {
    pub endpoint: String,
    pub api_key: Option<String>,
    /// Names this client to the server, so the voices it uploads stay its
    /// own. 16 to 64 hex digits; empty means "send none" (no custom voices).
    pub client_id: String,
    /// Which of the server's engines to speak with. `None` = its default.
    pub engine: Option<String>,
    /// A voice of that engine, or `custom:<name>` for one of this client's
    /// own clips. `None` = the engine's default on the server.
    pub voice: Option<String>,
}

/// The header that carries [`ServerLink::client_id`].
const CLIENT_HEADER: &str = "x-voxctrl-client";

/// The prefix that marks a voice as one of the client's own clips.
pub const CUSTOM_VOICE_PREFIX: &str = "custom:";

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

/// The key, and the client id when there is one.
fn signed(req: reqwest::blocking::RequestBuilder, link: &ServerLink) -> reqwest::blocking::RequestBuilder {
    let req = with_key(req, link.api_key.as_deref());
    match link.client_id.trim() {
        "" => req,
        id => req.header(CLIENT_HEADER, id),
    }
}

/// `/v1/voices` or `/v1/voices/<name>` for a speech endpoint.
fn voices_url(endpoint: &str, name: Option<&str>) -> String {
    let base = speech_url(endpoint);
    let base = base.strip_suffix("/audio/speech").unwrap_or(&base);
    match name {
        // Names are letters, digits, spaces, '-' and '_': only the space
        // needs escaping.
        Some(name) => format!("{base}/voices/{}", name.replace(' ', "%20")),
        None => format!("{base}/voices"),
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

/// Have the server speak `text` and return the WAV. Blocking. Errors are
/// worded for the user.
pub fn fetch_speech(link: &ServerLink, text: &str) -> Result<Vec<u8>> {
    fetch_speech_unless(link, text, &|| false)?.ok_or_else(|| anyhow!("The speech request was cancelled."))
}

/// How often a waiting speech request asks whether it is still wanted.
const CANCEL_CHECK: Duration = Duration::from_millis(40);

/// [`fetch_speech`], given up as soon as `cancelled` says so: `Ok(None)`.
///
/// Giving up closes the connection, which is how the server learns that
/// nobody is waiting and can skip the work. Without that, speech the user had
/// already stopped kept this thread waiting, sometimes for many seconds, and
/// kept the server synthesizing audio that would never be played while the
/// next thing they said queued up behind it.
pub fn fetch_speech_unless(
    link: &ServerLink,
    text: &str,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Option<Vec<u8>>> {
    if link.endpoint.trim().is_empty() {
        return Err(anyhow!(
            "No server address is set. Enter it under Settings → Engine → Remote Speech Engine Settings."
        ));
    }
    let mut body = serde_json::json!({ "input": text, "response_format": "wav" });
    if let Some(engine) = link.engine.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        body["model"] = engine.into();
    }
    if let Some(voice) = link.voice.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
        body["voice"] = voice.into();
    }

    let request = async {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(SPEECH_TIMEOUT)
            .build()
            .context("Could not set up the connection")?;
        let mut req = http.post(speech_url(&link.endpoint)).json(&body);
        if let Some(key) = link.api_key.as_deref().map(str::trim).filter(|k| !k.is_empty()) {
            req = req.bearer_auth(key);
        }
        if !link.client_id.trim().is_empty() {
            req = req.header(CLIENT_HEADER, link.client_id.trim());
        }
        let response = req.send().await.map_err(|e| {
            if e.is_connect() {
                anyhow!("Could not reach the server to speak. Check that it is running and the address is right.")
            } else if e.is_timeout() {
                anyhow!("The server took too long to speak.")
            } else {
                anyhow!("The request to the server failed: {e}")
            }
        })?;
        let status = response.status();
        let bytes = response.bytes().await.context("The server's audio could not be read")?;
        Ok::<_, anyhow::Error>((status, bytes))
    };

    // On a thread of its own, so this works whatever kind of thread calls it.
    let outcome = std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("Could not start the connection")?;
                runtime.block_on(async {
                    tokio::pin!(request);
                    loop {
                        tokio::select! {
                            result = &mut request => return result.map(Some),
                            _ = tokio::time::sleep(CANCEL_CHECK) => {
                                if cancelled() {
                                    // Dropping the request here closes the connection.
                                    return Ok(None);
                                }
                            }
                        }
                    }
                })
            })
            .join()
    });
    let Some((status, body)) = outcome.map_err(|_| anyhow!("The speech request stopped unexpectedly."))?? else {
        return Ok(None);
    };
    finish_speech_reply(status, &body).map(Some)
}

fn finish_speech_reply(status: reqwest::StatusCode, body: &[u8]) -> Result<Vec<u8>> {
    if !status.is_success() {
        return Err(match (status.as_u16(), error_message(body)) {
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

/// One engine a server offers.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ServerEngine {
    pub id: String,
    pub name: String,
    /// The engine's own voices that can be picked by name.
    pub voices: Vec<String>,
    /// The voice the server uses when none is picked.
    pub default_voice: Option<String>,
    /// Whether it can speak with a clip this client uploads.
    pub custom_voices: bool,
}

/// Whether a server offers text-to-speech, in words for the Settings window.
#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct ServerTtsStatus {
    pub available: bool,
    /// The server's default engine and its voice, when it says.
    pub engine: Option<String>,
    pub voice: Option<String>,
    /// What to tell the user, including what to do when it is not available.
    pub message: String,
    /// Every engine on offer, the default first. Empty from a server older
    /// than 0.9.11, which has the one engine named above and no choice.
    #[serde(default)]
    pub engines: Vec<ServerEngine>,
    /// Whether the server stores voices clients upload.
    #[serde(default)]
    pub custom_voices: bool,
}

impl ServerTtsStatus {
    fn unavailable(message: impl Into<String>) -> Self {
        Self { message: message.into(), ..Default::default() }
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
    let response = match signed(client.get(speech_url(&link.endpoint)), link).send() {
        Ok(r) => r,
        Err(e) if e.is_connect() => return ServerTtsStatus::unavailable("Could not connect to the server."),
        Err(e) if e.is_timeout() => return ServerTtsStatus::unavailable("The server did not answer in time."),
        Err(_) => return ServerTtsStatus::unavailable("The request to the server failed."),
    };
    let status = response.status().as_u16();
    let body = response.bytes().unwrap_or_default();
    interpret_status(status, &body)
}

// ── This client's own voices ──────────────────────────────────────────────────

/// The id this computer and user go by on a server, the same every run: a
/// hash of the machine's id and the user name, so nothing identifying is
/// sent. It only has to keep one client's voices apart from another's.
pub fn default_client_id() -> String {
    let machine = ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .map(|id| id.trim().to_string())
        .filter(|id| !id.is_empty())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_default();
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default();
    client_id_from(&machine, &user)
}

fn client_id_from(machine: &str, user: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(b"voxctrl-client\0");
    hasher.update(machine.as_bytes());
    hasher.update([0]);
    hasher.update(user.as_bytes());
    hex::encode(&hasher.finalize()[..16])
}

/// A voice clip on this computer: `<name>.wav`, with `<name>.txt` beside it
/// when it has a transcript.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalVoice {
    pub name: String,
    pub wav: std::path::PathBuf,
    pub transcript: Option<String>,
    /// Changes when the clip or its transcript does.
    pub fingerprint: String,
}

/// A name the server will take: it becomes a file name there.
pub fn uploadable_voice_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.trim() == name
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ' '))
}

/// The voice clips in `dir`, by name. Clips whose names the server would not
/// take are left out; they still work with this computer's own engines.
pub fn local_custom_voices(dir: &std::path::Path) -> Vec<LocalVoice> {
    use sha2::Digest;
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut voices: Vec<LocalVoice> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("wav")))
        .filter_map(|wav| {
            let name = wav.file_stem()?.to_str()?.to_string();
            if !uploadable_voice_name(&name) {
                return None;
            }
            let bytes = std::fs::read(&wav).ok()?;
            let transcript = std::fs::read_to_string(wav.with_extension("txt"))
                .ok()
                .map(|t| t.trim().to_string())
                .filter(|t| !t.is_empty());
            let mut hasher = sha2::Sha256::new();
            hasher.update(&bytes);
            hasher.update([0]);
            hasher.update(transcript.as_deref().unwrap_or_default().as_bytes());
            Some(LocalVoice { name, wav, transcript, fingerprint: hex::encode(&hasher.finalize()[..16]) })
        })
        .collect();
    voices.sort_by(|a, b| a.name.cmp(&b.name));
    voices
}

/// What [`sync_voices`] changed on the server.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
pub struct VoiceSync {
    pub uploaded: Vec<String>,
    pub removed: Vec<String>,
}

fn reply_error(what: &str, status: reqwest::StatusCode, body: &[u8]) -> anyhow::Error {
    match error_message(body) {
        Some(message) => anyhow!("{what} (HTTP {}): {message}", status.as_u16()),
        None => anyhow!("{what} (HTTP {}).", status.as_u16()),
    }
}

/// The voices this client has on the server: `(name, fingerprint)`.
fn server_voices(http: &reqwest::blocking::Client, link: &ServerLink) -> Result<Vec<(String, String)>> {
    let response = signed(http.get(voices_url(&link.endpoint, None)), link)
        .send()
        .context("Could not reach the server to check your voices")?;
    let status = response.status();
    let body = response.bytes().context("The server's list of voices could not be read")?;
    if !status.is_success() {
        return Err(reply_error("The server would not list your voices", status, &body));
    }
    let value: serde_json::Value = serde_json::from_slice(&body).context("The server's list of voices was not JSON")?;
    Ok(value
        .get("voices")
        .and_then(|v| v.as_array())
        .map(|voices| {
            voices
                .iter()
                .filter_map(|v| {
                    Some((v.get("name")?.as_str()?.to_string(), v.get("fingerprint")?.as_str().unwrap_or("").to_string()))
                })
                .collect()
        })
        .unwrap_or_default())
}

fn upload_voice(http: &reqwest::blocking::Client, link: &ServerLink, voice: &LocalVoice) -> Result<()> {
    let clip = std::fs::read(&voice.wav).with_context(|| format!("Could not read {}", voice.wav.display()))?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .part("audio", reqwest::blocking::multipart::Part::bytes(clip).file_name("voice.wav"))
        .text("fingerprint", voice.fingerprint.clone());
    if let Some(transcript) = &voice.transcript {
        form = form.text("transcript", transcript.clone());
    }
    let response = signed(http.put(voices_url(&link.endpoint, Some(&voice.name))), link)
        .multipart(form)
        .send()
        .with_context(|| format!("Could not send the voice \"{}\" to the server", voice.name))?;
    let status = response.status();
    let body = response.bytes().unwrap_or_default();
    if !status.is_success() {
        return Err(reply_error(&format!("The server did not accept the voice \"{}\"", voice.name), status, &body));
    }
    Ok(())
}

/// Make the server's copy of this client's voices match `local`: upload the
/// new and changed ones, remove the ones that are no longer here. So a voice
/// deleted on this computer disappears from the server at the next sync.
/// Blocking.
pub fn sync_voices(link: &ServerLink, local: &[LocalVoice]) -> Result<VoiceSync> {
    anyhow::ensure!(!link.client_id.trim().is_empty(), "This computer has no client id to send voices under.");
    let http = client(Duration::from_secs(60))?;
    let on_server = server_voices(&http, link)?;
    let mut report = VoiceSync::default();
    for voice in local {
        let current = on_server.iter().any(|(name, fp)| name == &voice.name && fp == &voice.fingerprint);
        if !current {
            upload_voice(&http, link, voice)?;
            report.uploaded.push(voice.name.clone());
        }
    }
    for (name, _) in &on_server {
        if !local.iter().any(|v| &v.name == name) {
            let response = signed(http.delete(voices_url(&link.endpoint, Some(name))), link)
                .send()
                .with_context(|| format!("Could not remove the voice \"{name}\" from the server"))?;
            if !response.status().is_success() {
                let status = response.status();
                let body = response.bytes().unwrap_or_default();
                return Err(reply_error(&format!("The server did not remove the voice \"{name}\""), status, &body));
            }
            report.removed.push(name.clone());
        }
    }
    Ok(report)
}

fn interpret_status(status: u16, body: &[u8]) -> ServerTtsStatus {
    match status {
        200 => {
            let value: serde_json::Value = serde_json::from_slice(body).unwrap_or_default();
            let text = |key: &str| value.get(key).and_then(|v| v.as_str()).map(str::to_string);
            let (engine, voice) = (text("engine"), text("voice"));
            match value.get("available").and_then(|a| a.as_bool()) {
                Some(true) => {
                    let engines: Vec<ServerEngine> =
                        value.get("engines").and_then(|e| serde_json::from_value(e.clone()).ok()).unwrap_or_default();
                    ServerTtsStatus {
                        available: true,
                        message: match (engines.len(), &engine, &voice) {
                            (n, _, _) if n > 1 => format!("This server offers {n} speech engines."),
                            (_, Some(engine), Some(voice)) => format!("This server speaks with {engine}, voice {voice}."),
                            (_, Some(engine), None) => format!("This server speaks with {engine}."),
                            _ => "This server offers text-to-speech.".into(),
                        },
                        engine,
                        voice,
                        engines,
                        custom_voices: value.get("custom_voices").and_then(|c| c.as_bool()).unwrap_or(false),
                    }
                }
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
    fn a_client_id_is_stable_distinct_per_user_and_one_the_server_accepts() {
        let id = client_id_from("0f3a9c", "josh");
        assert_eq!(id, client_id_from("0f3a9c", "josh"));
        assert_ne!(id, client_id_from("0f3a9c", "sam"));
        assert_ne!(id, client_id_from("77bb11", "josh"));
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert!(!id.contains("josh") && !id.contains("0f3a9c"));
        assert_eq!(default_client_id().len(), 32);
    }

    #[test]
    fn voice_urls_sit_beside_the_speech_url() {
        assert_eq!(voices_url("http://box:8787/v1", None), "http://box:8787/v1/voices");
        assert_eq!(voices_url("http://box:8787", Some("my voice")), "http://box:8787/v1/voices/my%20voice");
    }

    #[test]
    fn the_engines_a_server_offers_are_read_and_an_old_server_has_none() {
        let new = interpret_status(
            200,
            br#"{"available": true, "engine": "kitten_tts_2", "voice": "Bruno", "custom_voices": true, "engines": [
                {"id": "kitten_tts_2", "name": "Kitten TTS 2", "voices": ["Bruno", "Luna"], "default_voice": "Bruno", "custom_voices": true},
                {"id": "piper", "name": "Piper", "voices": ["en-us-amy-low"], "default_voice": "en-us-amy-low", "custom_voices": false}]}"#,
        );
        assert_eq!(new.message, "This server offers 2 speech engines.");
        assert_eq!(new.engines[1].id, "piper");
        assert!(new.engines[0].custom_voices && new.custom_voices);

        let old = interpret_status(200, br#"{"available": true, "engine": "piper", "voice": "en-us-amy-low"}"#);
        assert!(old.available && old.engines.is_empty() && !old.custom_voices);
    }

    #[test]
    fn local_voices_are_found_with_their_transcripts_and_a_fingerprint_that_follows_changes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Rick.wav"), b"RIFFaaaa").unwrap();
        std::fs::write(dir.path().join("Rick.txt"), " hello there \n").unwrap();
        std::fs::write(dir.path().join("Plain.WAV"), b"RIFFbbbb").unwrap();
        // Not uploadable under these names, and not a clip at all.
        std::fs::write(dir.path().join("v1.2 final.wav"), b"RIFF").unwrap();
        std::fs::write(dir.path().join("notes.txt"), b"x").unwrap();

        let voices = local_custom_voices(dir.path());
        assert_eq!(voices.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), vec!["Plain", "Rick"]);
        assert_eq!(voices[1].transcript.as_deref(), Some("hello there"));
        assert_eq!(voices[0].transcript, None);
        assert_eq!(voices[1].fingerprint.len(), 32);

        let before = voices[1].fingerprint.clone();
        std::fs::write(dir.path().join("Rick.txt"), "different words").unwrap();
        assert_ne!(local_custom_voices(dir.path())[1].fingerprint, before, "a new transcript is a new version");
        std::fs::write(dir.path().join("Rick.txt"), " hello there \n").unwrap();
        assert_eq!(local_custom_voices(dir.path())[1].fingerprint, before);

        assert!(local_custom_voices(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn no_address_is_explained_without_a_request() {
        let link = ServerLink { endpoint: "  ".into(), ..Default::default() };
        assert!(fetch_speech(&link, "hi").unwrap_err().to_string().contains("No server address"));
        assert!(check_server_tts(&link).message.contains("No server address"));
    }

    #[test]
    fn an_unreachable_server_is_said_plainly() {
        let link = ServerLink { endpoint: "http://127.0.0.1:1/v1".into(), ..Default::default() };
        assert!(fetch_speech(&link, "hi").unwrap_err().to_string().contains("Could not reach the server"));
        assert_eq!(check_server_tts(&link).message, "Could not connect to the server.");
    }
}
