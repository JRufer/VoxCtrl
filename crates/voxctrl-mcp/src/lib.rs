//! JSON-RPC 2.0 MCP server over a Unix domain socket (Linux) or a named pipe (Windows).
//!
//! Exposed tools:
//!   - transcribe_voice(timeout_seconds) → {text}
//!   - speak_text(text) → {}
//!   - get_status() → {recording, speaking}
//!   - set_target(target_id?) → label, or the list of targets
//!   - get_recent_transcripts(limit?, query?) → [{at, target, text}], listed
//!     only when the user allowed MCP access to history

#[cfg(not(target_os = "windows"))]
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info};
#[cfg(not(target_os = "windows"))]
use tracing::warn;

/// Where the MCP server listens, in the form clients need.
///
/// The server has always bound the right thing per platform — a Unix socket on
/// Linux, a named pipe on Windows — but this constant said `/tmp` regardless,
/// and it is what the app, the README and `docs/architecture.md` quote to the
/// user when telling an MCP client where to connect. On Windows that address
/// does not exist.
#[cfg(target_os = "windows")]
pub const SOCKET_PATH: &str = r"\\.\pipe\voxctrl-mcp";
/// The address VoxCtrl has always advertised on Linux. It is now a link to
/// [`socket_path`], kept so existing MCP client configs keep working.
#[cfg(not(target_os = "windows"))]
pub const SOCKET_PATH: &str = LEGACY_SOCKET_PATH;

/// The pre-0.8.5 socket location, in the shared `/tmp`.
pub const LEGACY_SOCKET_PATH: &str = "/tmp/voxctrl-mcp.sock";

/// Where the Linux server's socket actually lives: the user's private
/// runtime folder (`$XDG_RUNTIME_DIR`, normally `/run/user/<uid>`, which only
/// that user can enter), falling back to `/tmp` without one.
///
/// In the shared `/tmp`, another account on the machine could create
/// `voxctrl-mcp.sock` first, while VoxCtrl isn't running, and pose as the
/// server: an MCP client would then hand it whatever it asked VoxCtrl to
/// speak, and take back whatever "transcript" it chose to return. A name
/// inside the runtime folder can't be taken by anyone else.
#[cfg(not(target_os = "windows"))]
pub fn socket_path() -> PathBuf {
    runtime_socket_path(std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from))
}

#[cfg(not(target_os = "windows"))]
fn runtime_socket_path(runtime_dir: Option<PathBuf>) -> PathBuf {
    match runtime_dir {
        Some(dir) if dir.is_absolute() && dir.is_dir() => dir.join("voxctrl-mcp.sock"),
        _ => PathBuf::from(LEGACY_SOCKET_PATH),
    }
}

// ── JSON-RPC types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    id: Option<Value>,
    method: String,
    params: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

impl JsonRpcResponse {
    fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    fn err(id: Option<Value>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
            }),
        }
    }
}

// ── App callbacks ─────────────────────────────────────────────────────────────

/// One dictation from the history, as `get_recent_transcripts` returns it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Transcript {
    /// When it was dictated, RFC 3339.
    pub at: String,
    /// Where it was delivered.
    pub target: String,
    pub text: String,
}

/// An output target, as `set_target` lists them.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TargetInfo {
    pub id: String,
    pub label: String,
}

/// Callbacks that the MCP server uses to interact with the app coordinator.
pub trait McpCallbacks: Send + Sync + 'static {
    /// Start recording and return the transcribed text (blocks until done or timeout).
    fn transcribe_voice(&self, timeout_secs: f64) -> impl std::future::Future<Output = Result<String>> + Send;

    /// Queue text for TTS playback.
    fn speak_text(&self, text: String) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Get current status flags.
    fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send;

    /// The user's recent dictations, newest first, at most `limit`, those
    /// containing `query` when it isn't empty. `None` when the user hasn't
    /// allowed MCP clients to read history (History window → Privacy settings), which is
    /// the default; the tool is then not even listed.
    fn recent_transcripts(
        &self,
        limit: usize,
        query: String,
    ) -> impl std::future::Future<Output = Option<Vec<Transcript>>> + Send;

    /// The output targets a dictation can be sent to.
    fn list_targets(&self) -> impl std::future::Future<Output = Vec<TargetInfo>> + Send;

    /// Send the next dictations to `target_id` (until a hotkey picks another
    /// target). Returns the target's label; errors for an unknown id.
    fn set_target(
        &self,
        target_id: String,
    ) -> impl std::future::Future<Output = Result<String>> + Send;

    /// The user's configured recording timeout (Settings → General → Record
    /// timeout), in seconds. Used for `transcribe_voice` whenever the caller
    /// does not pass an explicit `timeout_seconds`, and advertised as the
    /// default in the tool schema. Read per call so a change in Settings
    /// applies without restarting the server.
    fn default_record_timeout(&self) -> impl std::future::Future<Output = f64> + Send;
}

// ── Server ────────────────────────────────────────────────────────────────────

pub async fn run_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    #[cfg(target_os = "linux")]
    run_unix_server(callbacks).await?;

    #[cfg(target_os = "windows")]
    run_named_pipe_server(callbacks).await?;

    Ok(())
}

// ── Unix socket server (Linux) ────────────────────────────────────────────────

/// Point the old `/tmp` address at the real socket, so MCP clients set up
/// before the move keep connecting. Only an entry this user owns is ever
/// replaced; one belonging to someone else is left alone (and reported), so
/// a squatter can't be "fixed" into a hijack. The socket itself stays 0600,
/// so the link grants nothing the socket doesn't.
#[cfg(target_os = "linux")]
fn link_legacy_path(real: &std::path::Path, legacy: &std::path::Path) {
    use std::os::unix::fs::MetadataExt;
    if real == legacy {
        return;
    }
    if let Ok(meta) = std::fs::symlink_metadata(legacy) {
        // SAFETY: getuid has no preconditions and cannot fail.
        let me = unsafe { libc::getuid() };
        if meta.uid() != me {
            warn!(
                "{} belongs to another user; MCP clients must use {}",
                legacy.display(),
                real.display()
            );
            return;
        }
        let _ = std::fs::remove_file(legacy);
    }
    if let Err(e) = std::os::unix::fs::symlink(real, legacy) {
        debug!("Could not link {} to the MCP socket: {e}", legacy.display());
    }
}

#[cfg(target_os = "linux")]
async fn run_unix_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    use tokio::net::UnixListener;

    let path = socket_path();
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path)?;
    // Restrict to the owning user only so other local users cannot
    // activate the microphone or issue TTS via the MCP socket.
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    info!("MCP server listening on {}", path.display());
    link_legacy_path(&path, std::path::Path::new(LEGACY_SOCKET_PATH));

    loop {
        match listener.accept().await {
            Ok((stream, _)) => {
                let cb = callbacks.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, cb).await {
                        debug!("MCP connection error: {e}");
                    }
                });
            }
            Err(e) => warn!("MCP accept error: {e}"),
        }
    }
}

async fn handle_connection<S, C>(stream: S, callbacks: Arc<C>) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
    C: McpCallbacks,
{
    let (reader, mut writer) = tokio::io::split(stream);
    let mut lines = BufReader::new(reader).lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }
        debug!("MCP ← {line}");
        if let Some(response) = dispatch(&line, &callbacks).await {
            let out = serde_json::to_string(&response)? + "\n";
            debug!("MCP → {out}");
            writer.write_all(out.as_bytes()).await?;
        }
    }
    Ok(())
}

// ── Windows named pipe server ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
async fn run_named_pipe_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    // Windows named pipes use the \\.\pipe\ namespace
    // tokio supports this via tokio::net::windows::named_pipe
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    let pipe_name = SOCKET_PATH;
    info!("MCP server listening on {pipe_name}");

    loop {
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .create(pipe_name)?;

        server.connect().await?;

        let cb = callbacks.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(server, cb).await {
                debug!("MCP connection error: {e}");
            }
        });
    }
}

// ── Dispatcher ────────────────────────────────────────────────────────────────

async fn dispatch<C: McpCallbacks>(raw: &str, cb: &Arc<C>) -> Option<JsonRpcResponse> {
    let req: JsonRpcRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => return Some(JsonRpcResponse::err(None, -32700, format!("Parse error: {e}"))),
    };

    let id = req.id.clone();

    // If it's a notification (no id) and NOT initialize, do not return a response
    if id.is_none() && req.method != "initialize" {
        return None;
    }

    let response = match req.method.as_str() {
        "initialize" => {
            JsonRpcResponse::ok(id, json!({
                "protocolVersion": "2024-11-05",
                "serverInfo": {
                    "name": "voxctrl",
                    "version": "1.0.0"
                },
                "capabilities": {
                    "tools": {}
                }
            }))
        }
        "notifications/initialized" => {
            JsonRpcResponse::ok(id, json!({}))
        }
        "tools/list" => {
            let history = cb.recent_transcripts(0, String::new()).await.is_some();
            JsonRpcResponse::ok(id, get_tool_list(cb.default_record_timeout().await, history))
        }
        "tools/call" => {
            let name = req.params.as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let args = req.params.as_ref()
                .and_then(|p| p.get("arguments"));

            match call_tool(name, args, cb).await {
                Ok(res) => JsonRpcResponse::ok(id, res),
                Err(e) => JsonRpcResponse::err(id, -32603, e.to_string()),
            }
        }
        other => {
            JsonRpcResponse::err(id, -32601, format!("Method not found: {other}"))
        }
    };

    Some(response)
}

// ── Tool List Definition ──────────────────────────────────────────────────────

fn get_tool_list(default_record_timeout: f64, history_access: bool) -> Value {
    let mut list = json!({
        "tools": [
            {
                "name": "transcribe_voice",
                "description": "Records the user's voice through their microphone and returns the transcribed text.\n\nHOW IT WORKS:\n  Calling this tool immediately activates the user's microphone. The user speaks, and when they stop (or the timeout is reached) the audio is transcribed locally using Whisper and the text is returned. The microphone indicator in the app's system tray will show a recording state while this tool is active.\n\nWHEN TO USE:\n  - Whenever you need a spoken response or clarification from the user.\n  - To conduct a voice-driven conversation: speak a question with speak_text, then call transcribe_voice to capture the answer.\n  - When the user has indicated they prefer to respond by voice rather than typing.\n  - To capture dictated content such as notes, messages, or commands.\n\nWHEN NOT TO USE:\n  - Do not call while get_status shows recording=true (a recording is already in progress). Check status first if unsure.\n  - Do not call while get_status shows speaking=true; wait for TTS to finish so the microphone does not pick up the synthesised voice.\n  - Do not loop rapidly on empty results; if '(no speech detected)' is returned twice in a row, inform the user and wait for a typed prompt instead.\n\nPARAMETERS:\n  timeout_seconds (number, optional): Maximum wall-clock seconds to wait for the user to finish speaking. Use a shorter value (5–8 s) for quick yes/no questions. Use a longer value (30–60 s) when asking the user to dictate a paragraph or give detailed instructions.\n\nRETURN VALUE:\n  A plain-text string containing the transcribed speech. If no speech was detected within the timeout the string will be '(no speech detected)'. The transcript may contain minor errors from the speech model; treat it as lightly noisy text and correct obvious errors from context before acting on it.\n\nEXAMPLE FLOW — voice Q&A:\n  1. speak_text(\"What city are you in?\")\n  2. transcribe_voice(timeout_seconds=10)  → \"I'm in Seattle\"\n  3. Use 'Seattle' in subsequent tool calls or responses.\n\nEXAMPLE FLOW — voice dictation:\n  1. speak_text(\"Please dictate your message now.\")\n  2. transcribe_voice(timeout_seconds=45)  → full dictated message\n  3. Present the transcript back to the user for confirmation before sending.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "timeout_seconds": {
                            "type": "number",
                            "description": format!("Maximum seconds to wait for the user to finish speaking. Defaults to {default_record_timeout} (the Record timeout set in VoxCtrl Settings). Use 5–8 for short answers, 30–60 for dictation.")
                        }
                    }
                }
            },
            {
                "name": "speak_text",
                "description": "Converts text to speech and plays it aloud through the user's speakers.\n\nHOW IT WORKS:\n  The text is queued for playback using the locally configured TTS engine (piper neural TTS by default, espeak-ng as fallback). Playback happens asynchronously — this tool returns immediately while audio plays in the background. The app's system tray will show a speaking indicator. Only one utterance plays at a time; additional calls are queued and played in order.\n\nWHEN TO USE:\n  - To read your response aloud so the user does not have to look at the screen.\n  - To prompt the user before calling transcribe_voice (speak the question, then listen).\n  - To confirm actions: \"Done — I've sent your message.\"\n  - For accessibility: whenever the user has indicated they prefer audio output.\n  - To narrate step-by-step instructions the user can follow hands-free.\n\nWHEN NOT TO USE:\n  - Do not pass extremely long text (>500 words) in a single call; break long content into logical paragraphs and call speak_text once per paragraph so the user can interrupt between them.\n  - Do not include markdown syntax (**, ##, -, etc.) — it will be read aloud literally. Strip formatting before speaking.\n  - Do not include URLs, file paths, or code snippets; summarise them in plain prose instead.\n  - If get_status shows speaking=true and you need to ask a follow-up question, queue the next speak_text call; do not call transcribe_voice until speaking=false.\n\nPARAMETERS:\n  text (string, required): The plain-text content to speak. Should be natural prose — complete sentences, no markdown, no raw symbols. Punctuation is used by the TTS engine for pacing; commas and periods produce natural pauses.\n\nRETURN VALUE:\n  Returns the string 'spoken' when the text has been successfully queued for playback. This does NOT mean playback is complete — use get_status to check speaking=false if you need to wait before recording.\n\nEXAMPLE — confirm then record:\n  1. speak_text(\"I'll listen for your answer. Go ahead.\")\n  2. # Poll until speaking=false before recording\n  3. transcribe_voice(timeout_seconds=15)\n\nEXAMPLE — multi-part narration:\n  1. speak_text(\"Here are your three reminders for today.\")\n  2. speak_text(\"First: team standup at 9 AM.\")\n  3. speak_text(\"Second: review the pull request from Jordan.\")\n  4. speak_text(\"Third: submit your expense report by 5 PM.\")",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Plain-text content to speak aloud. Use natural prose with punctuation for pacing. No markdown, no code, no URLs."
                        }
                    },
                    "required": ["text"]
                }
            },
            {
                "name": "get_status",
                "description": "Returns the current state of the voice interface as a JSON object.\n\nHOW IT WORKS:\n  Queries the running VoxCtrl app for its live recording and TTS state. This is a lightweight, non-blocking call that returns immediately.\n\nWHEN TO USE:\n  - Before calling transcribe_voice: confirm recording=false so you do not start a second recording session while one is already active.\n  - Before calling transcribe_voice after speak_text: confirm speaking=false so the microphone does not capture TTS audio.\n  - To implement a wait loop: poll get_status every 1–2 seconds until speaking=false before proceeding to record.\n  - To diagnose unexpected behaviour: if transcribe_voice returns no speech, check whether the app is still in a speaking state.\n\nRETURN VALUE:\n  A JSON object with the following fields:\n    recording (boolean): true while the microphone is actively capturing audio for transcription. Only one recording session can run at a time.\n    speaking (boolean): true while TTS audio is currently playing through the speakers. The queue may contain additional utterances that will play after the current one finishes.\n\nEXAMPLE RESPONSE:\n  {\"recording\": false, \"speaking\": false}  — idle, safe to record or speak\n  {\"recording\": true,  \"speaking\": false}  — recording in progress\n  {\"recording\": false, \"speaking\": true}   — TTS playing, wait before recording\n\nRECOMMENDED PATTERN — speak then record safely:\n  1. speak_text(\"Your question here.\")\n  2. Loop: get_status → if speaking=true, wait 1 s and repeat\n  3. transcribe_voice(timeout_seconds=15)\n\nNOTE: Both fields can briefly be false between a speak_text call returning and the audio actually starting. If precise synchronisation matters, add a short delay (0.5 s) after speak_text before beginning to poll.",
                "inputSchema": {
                    "type": "object",
                    "properties": {}
                }
            },
            {
                "name": "set_target",
                "description": "Chooses where the user's next dictations are delivered: typed into the focused window, appended to a file, sent to a webhook, and so on, as set up in VoxCtrl's Output Commands. Applies to transcribe_voice results and to the user's own dictation until they press a hotkey bound to another target.\n\nPARAMETERS:\n  target_id (string, optional): The id of the target. Omit it to list the available targets without changing anything.\n\nRETURN VALUE:\n  The label of the selected target, or a JSON list of {id, label} when target_id is omitted. An unknown id is an error that lists the valid ones.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target_id": {
                            "type": "string",
                            "description": "Output target id. Omit to list targets."
                        }
                    }
                }
            }
        ]
    });
    if history_access {
        list["tools"].as_array_mut().expect("tools is an array").push(json!({
            "name": "get_recent_transcripts",
            "description": "Returns the user's recent dictations from VoxCtrl's history, newest first, as a JSON list of {at, target, text}. Only available because the user allowed it in VoxCtrl's History window. Dictations can be private: use them only for what the user asked you to do.\n\nPARAMETERS:\n  limit (number, optional): How many to return, 1–50. Default 10.\n  query (string, optional): Only dictations containing this text (case-insensitive).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": { "type": "number", "description": "How many to return, 1–50. Default 10." },
                    "query": { "type": "string", "description": "Only dictations containing this text." }
                }
            }
        }));
    }
    list
}

// ── Tool Call Execution ───────────────────────────────────────────────────────

async fn call_tool<C: McpCallbacks>(name: &str, args: Option<&Value>, cb: &Arc<C>) -> Result<Value> {
    match name {
        "transcribe_voice" => {
            let timeout = match args.and_then(|a| a.get("timeout_seconds")).and_then(|t| t.as_f64()) {
                Some(t) => t,
                None => cb.default_record_timeout().await,
            };

            let text = cb.transcribe_voice(timeout).await?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": if text.is_empty() { "(no speech detected)".to_string() } else { text }
                    }
                ]
            }))
        }
        "speak_text" => {
            let text = args
                .and_then(|a| a.get("text"))
                .and_then(|t| t.as_str())
                .ok_or_else(|| anyhow::anyhow!("speak_text requires 'text' argument"))?
                .to_string();

            cb.speak_text(text).await?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": "spoken"
                    }
                ]
            }))
        }
        "get_status" => {
            let (recording, speaking) = cb.get_status().await;
            let status = json!({
                "recording": recording,
                "speaking": speaking
            });
            let status_str = serde_json::to_string(&status)?;
            Ok(json!({
                "content": [
                    {
                        "type": "text",
                        "text": status_str
                    }
                ]
            }))
        }
        "set_target" => {
            let wanted = args
                .and_then(|a| a.get("target_id"))
                .and_then(|t| t.as_str());
            let text = match wanted {
                None => serde_json::to_string(&cb.list_targets().await)?,
                Some(id) => cb.set_target(id.to_string()).await?,
            };
            Ok(json!({ "content": [ { "type": "text", "text": text } ] }))
        }
        "get_recent_transcripts" => {
            let limit = args
                .and_then(|a| a.get("limit"))
                .and_then(|l| l.as_f64())
                .map_or(10, |l| l.clamp(1.0, 50.0) as usize);
            let query = args
                .and_then(|a| a.get("query"))
                .and_then(|q| q.as_str())
                .unwrap_or("")
                .to_string();
            let transcripts = cb.recent_transcripts(limit, query).await.ok_or_else(|| {
                anyhow::anyhow!(
                    "Reading dictation history is turned off. The user can allow it in VoxCtrl's History window (tray menu → History → Privacy settings)."
                )
            })?;
            Ok(
                json!({ "content": [ { "type": "text", "text": serde_json::to_string(&transcripts)? } ] }),
            )
        }
        other => Err(anyhow::anyhow!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Records the timeout `transcribe_voice` was actually called with, and
    /// reports a configured default distinguishable from the old hard-coded
    /// 15.0 so a regression cannot pass by coincidence.
    struct FakeApp {
        configured_timeout: f64,
        /// Last timeout seen by `transcribe_voice`, as f64 bits.
        seen: AtomicU64,
        /// `None`: history access off, as by default.
        history: Option<Vec<Transcript>>,
        target: std::sync::Mutex<String>,
    }

    impl FakeApp {
        fn new(configured_timeout: f64) -> Arc<Self> {
            Arc::new(Self {
                configured_timeout,
                seen: AtomicU64::new(f64::NAN.to_bits()),
                history: None,
                target: std::sync::Mutex::new("default".into()),
            })
        }

        fn with_history(history: Vec<Transcript>) -> Arc<Self> {
            let mut app = Arc::try_unwrap(Self::new(15.0)).ok().unwrap();
            app.history = Some(history);
            Arc::new(app)
        }

        fn seen_timeout(&self) -> f64 {
            f64::from_bits(self.seen.load(Ordering::SeqCst))
        }
    }

    impl McpCallbacks for FakeApp {
        async fn transcribe_voice(&self, timeout_secs: f64) -> Result<String> {
            self.seen.store(timeout_secs.to_bits(), Ordering::SeqCst);
            Ok("hello".to_string())
        }

        async fn speak_text(&self, _text: String) -> Result<()> {
            Ok(())
        }

        async fn get_status(&self) -> (bool, bool) {
            (false, false)
        }

        async fn default_record_timeout(&self) -> f64 {
            self.configured_timeout
        }

        async fn recent_transcripts(&self, limit: usize, query: String) -> Option<Vec<Transcript>> {
            let all = self.history.clone()?;
            Some(
                all.into_iter()
                    .filter(|t| t.text.contains(&query))
                    .take(limit)
                    .collect(),
            )
        }

        async fn list_targets(&self) -> Vec<TargetInfo> {
            vec![
                TargetInfo {
                    id: "default".into(),
                    label: "Focused Window".into(),
                },
                TargetInfo {
                    id: "notes".into(),
                    label: "Notes".into(),
                },
            ]
        }

        async fn set_target(&self, target_id: String) -> Result<String> {
            let label = self
                .list_targets()
                .await
                .into_iter()
                .find(|t| t.id == target_id)
                .map(|t| t.label)
                .ok_or_else(|| anyhow::anyhow!("no target {target_id}"))?;
            *self.target.lock().unwrap() = target_id;
            Ok(label)
        }
    }

    fn call(name: &str, arguments: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments},
        })
        .to_string()
    }

    fn tool_names(app: &Arc<FakeApp>) -> Vec<String> {
        let raw = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
        let res = block_on(dispatch(&raw, app)).expect("no response");
        res.result.unwrap()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    fn text_of(res: JsonRpcResponse) -> String {
        res.result.expect("an error, not a result")["content"][0]["text"]
            .as_str()
            .unwrap()
            .to_string()
    }

    fn transcripts() -> Vec<Transcript> {
        vec![
            Transcript {
                at: "2026-10-27T10:00:00Z".into(),
                target: "Notes".into(),
                text: "buy milk".into(),
            },
            Transcript {
                at: "2026-10-27T09:00:00Z".into(),
                target: "Focused Window".into(),
                text: "hello there".into(),
            },
        ]
    }

    /// History is private unless the user says otherwise: by default the
    /// tool isn't even offered, and calling it anyway is refused.
    #[test]
    fn history_is_closed_to_mcp_by_default() {
        let app = FakeApp::new(15.0);
        assert!(!tool_names(&app).contains(&"get_recent_transcripts".to_string()));
        let res = block_on(dispatch(&call("get_recent_transcripts", json!({})), &app)).unwrap();
        let err = res.error.expect("must be refused");
        assert!(err.message.contains("turned off"), "{}", err.message);
        assert!(res.result.is_none());
    }

    #[test]
    fn history_is_readable_once_allowed() {
        let app = FakeApp::with_history(transcripts());
        assert!(tool_names(&app).contains(&"get_recent_transcripts".to_string()));
        let text = text_of(
            block_on(dispatch(
                &call("get_recent_transcripts", json!({"query": "milk"})),
                &app,
            ))
            .unwrap(),
        );
        let got: Vec<Value> = serde_json::from_str(&text).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["text"], "buy milk");
        assert_eq!(got[0]["target"], "Notes");
    }

    #[test]
    fn set_target_lists_selects_and_rejects() {
        let app = FakeApp::new(15.0);
        assert!(tool_names(&app).contains(&"set_target".to_string()));
        let listed = text_of(block_on(dispatch(&call("set_target", json!({})), &app)).unwrap());
        assert!(listed.contains("\"notes\""), "{listed}");
        assert_eq!(
            *app.target.lock().unwrap(),
            "default",
            "listing changes nothing"
        );

        let picked = text_of(
            block_on(dispatch(
                &call("set_target", json!({"target_id": "notes"})),
                &app,
            ))
            .unwrap(),
        );
        assert_eq!(picked, "Notes");
        assert_eq!(*app.target.lock().unwrap(), "notes");

        let res = block_on(dispatch(
            &call("set_target", json!({"target_id": "nope"})),
            &app,
        ))
        .unwrap();
        assert!(res.error.is_some());
        assert_eq!(*app.target.lock().unwrap(), "notes");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_socket_lives_in_the_private_runtime_folder() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            runtime_socket_path(Some(dir.path().into())),
            dir.path().join("voxctrl-mcp.sock")
        );
        assert_eq!(runtime_socket_path(None), PathBuf::from(LEGACY_SOCKET_PATH));
        assert_eq!(
            runtime_socket_path(Some("relative".into())),
            PathBuf::from(LEGACY_SOCKET_PATH)
        );
        assert_eq!(
            runtime_socket_path(Some(dir.path().join("missing"))),
            PathBuf::from(LEGACY_SOCKET_PATH)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_old_address_links_to_the_new_socket() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("voxctrl-mcp.sock");
        std::fs::write(&real, b"").unwrap();
        let legacy = dir.path().join("legacy.sock");
        // A stale entry of our own is replaced.
        std::fs::write(&legacy, b"stale").unwrap();
        link_legacy_path(&real, &legacy);
        assert_eq!(std::fs::read_link(&legacy).unwrap(), real);
        // And again, over the link left last time.
        link_legacy_path(&real, &legacy);
        assert_eq!(std::fs::read_link(&legacy).unwrap(), real);
    }

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn transcribe_request(arguments: Value) -> String {
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "transcribe_voice", "arguments": arguments},
        })
        .to_string()
    }

    #[test]
    fn omitted_timeout_uses_the_configured_record_timeout() {
        let app = FakeApp::new(42.0);
        let res = block_on(dispatch(&transcribe_request(json!({})), &app));

        assert!(res.is_some(), "request produced no response");
        assert_eq!(app.seen_timeout(), 42.0);
    }

    #[test]
    fn missing_arguments_object_still_uses_the_configured_timeout() {
        let app = FakeApp::new(7.5);
        let raw = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "tools/call",
            "params": {"name": "transcribe_voice"},
        })
        .to_string();

        block_on(dispatch(&raw, &app));
        assert_eq!(app.seen_timeout(), 7.5);
    }

    #[test]
    fn explicit_timeout_wins_over_the_configured_one() {
        let app = FakeApp::new(42.0);
        block_on(dispatch(
            &transcribe_request(json!({"timeout_seconds": 3.0})),
            &app,
        ));
        assert_eq!(app.seen_timeout(), 3.0);
    }

    #[test]
    fn tool_schema_advertises_the_configured_timeout() {
        let app = FakeApp::new(42.0);
        let raw = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}).to_string();
        let res = block_on(dispatch(&raw, &app)).expect("no response");

        let described = serde_json::to_string(&res.result.unwrap()).unwrap();
        assert!(
            described.contains("Defaults to 42"),
            "tools/list did not advertise the configured default: {described}"
        );
    }
}
