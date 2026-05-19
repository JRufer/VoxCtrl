//! JSON-RPC 2.0 MCP server over a Unix domain socket (Linux) or a named pipe (Windows).
//!
//! Exposed tools:
//!   - transcribe_voice(timeout_seconds) → {text}
//!   - speak_text(text) → {}
//!   - get_status() → {recording, speaking}

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{oneshot, Mutex};
use tracing::{debug, info, warn};

pub const SOCKET_PATH: &str = "/tmp/voxctl-mcp.sock";

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

/// Callbacks that the MCP server uses to interact with the app coordinator.
pub trait McpCallbacks: Send + Sync + 'static {
    /// Start recording and return the transcribed text (blocks until done or timeout).
    fn transcribe_voice(&self, timeout_secs: f64) -> impl std::future::Future<Output = Result<String>> + Send;

    /// Queue text for TTS playback.
    fn speak_text(&self, text: String) -> impl std::future::Future<Output = Result<()>> + Send;

    /// Get current status flags.
    fn get_status(&self) -> impl std::future::Future<Output = (bool, bool)> + Send;
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

#[cfg(target_os = "linux")]
async fn run_unix_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    use tokio::net::UnixListener;

    let path = PathBuf::from(SOCKET_PATH);
    if path.exists() {
        let _ = std::fs::remove_file(&path);
    }
    let listener = UnixListener::bind(&path)?;
    info!("MCP server listening on {SOCKET_PATH}");

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

#[cfg(target_os = "linux")]
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
        let response = dispatch(&line, &callbacks).await;
        let out = serde_json::to_string(&response)? + "\n";
        debug!("MCP → {out}");
        writer.write_all(out.as_bytes()).await?;
    }
    Ok(())
}

// ── Windows named pipe server ─────────────────────────────────────────────────

#[cfg(target_os = "windows")]
async fn run_named_pipe_server<C: McpCallbacks>(callbacks: Arc<C>) -> Result<()> {
    // Windows named pipes use the \\.\pipe\ namespace
    // tokio supports this via tokio::net::windows::named_pipe
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    const PIPE_NAME: &str = r"\\.\pipe\voxctl-mcp";
    info!("MCP server listening on {PIPE_NAME}");

    loop {
        let server = ServerOptions::new()
            .pipe_mode(PipeMode::Byte)
            .create(PIPE_NAME)?;

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

async fn dispatch<C: McpCallbacks>(raw: &str, cb: &Arc<C>) -> JsonRpcResponse {
    let req: JsonRpcRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => return JsonRpcResponse::err(None, -32700, format!("Parse error: {e}")),
    };

    let id = req.id.clone();

    match req.method.as_str() {
        "transcribe_voice" => {
            let timeout = req
                .params
                .as_ref()
                .and_then(|p| p["timeout_seconds"].as_f64())
                .unwrap_or(15.0);
            match cb.transcribe_voice(timeout).await {
                Ok(text) => JsonRpcResponse::ok(id, json!({ "text": text })),
                Err(e) => JsonRpcResponse::err(id, -32000, e.to_string()),
            }
        }
        "speak_text" => {
            let text = req
                .params
                .as_ref()
                .and_then(|p| p["text"].as_str())
                .unwrap_or("")
                .to_string();
            match cb.speak_text(text).await {
                Ok(_) => JsonRpcResponse::ok(id, json!({})),
                Err(e) => JsonRpcResponse::err(id, -32000, e.to_string()),
            }
        }
        "get_status" => {
            let (recording, speaking) = cb.get_status().await;
            JsonRpcResponse::ok(id, json!({ "recording": recording, "speaking": speaking }))
        }
        other => JsonRpcResponse::err(id, -32601, format!("Method not found: {other}")),
    }
}
