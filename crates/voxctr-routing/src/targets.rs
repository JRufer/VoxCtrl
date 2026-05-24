use anyhow::Context;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UnixStream};

use crate::models::{DeliveryResult, DeliveryType, OutputTarget, TestResult};

// ── Trait ─────────────────────────────────────────────────────────────────────

#[async_trait::async_trait]
pub trait DeliveryTarget: Send + Sync {
    async fn deliver(&self, text: &str) -> DeliveryResult;
    async fn test(&self) -> TestResult;
}

// ── Factory ───────────────────────────────────────────────────────────────────

pub fn build_target(config: OutputTarget) -> Box<dyn DeliveryTarget> {
    match config.delivery {
        DeliveryType::Inject    => Box::new(InjectTarget(config)),
        DeliveryType::Clipboard => Box::new(ClipboardTarget(config)),
        DeliveryType::Exec      => Box::new(ExecTarget(config)),
        DeliveryType::Pipe      => Box::new(PipeTarget(config)),
        DeliveryType::Socket    => Box::new(SocketTarget(config)),
        DeliveryType::File      => Box::new(FileTarget(config)),
        DeliveryType::Dbus      => Box::new(DbusTarget(config)),
        DeliveryType::Http      => Box::new(HttpTarget(config)),
        DeliveryType::Webhook   => Box::new(WebhookTarget(config)),
        DeliveryType::Mcp       => Box::new(McpTarget(config)),
    }
}

// ── InjectTarget ──────────────────────────────────────────────────────────────

pub struct InjectTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for InjectTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let mut payload = text.to_string();
        if self.0.append_newline {
            payload.push('\n');
        }

        #[cfg(target_os = "linux")]
        {
            let wayland = std::env::var("WAYLAND_DISPLAY").is_ok();
            if wayland && which("wtype") {
                let ok = tokio::process::Command::new("wtype")
                    .arg("--")
                    .arg(&payload)
                    .status()
                    .await
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return DeliveryResult::ok(payload);
                }
            }
            if which("xdotool") {
                let ok = tokio::process::Command::new("xdotool")
                    .args(["type", "--clearmodifiers", "--delay", "12", "--"])
                    .arg(&payload)
                    .status()
                    .await
                    .map(|s| s.success())
                    .unwrap_or(false);
                if ok {
                    return DeliveryResult::ok(payload);
                }
            }
            return DeliveryResult::err("No injection method available (wtype / xdotool)");
        }

        #[cfg(target_os = "windows")]
        {
            let script = format!(
                r#"Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.SendKeys]::SendWait("{}")"#,
                payload.replace('"', "\"\"")
            );
            let ok = tokio::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &script])
                .status()
                .await
                .map(|s| s.success())
                .unwrap_or(false);
            return if ok {
                DeliveryResult::ok(payload)
            } else {
                DeliveryResult::err("PowerShell SendKeys failed")
            };
        }

        #[allow(unreachable_code)]
        DeliveryResult::err("Text injection not supported on this platform")
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        {
            if which("wtype") {
                return TestResult { reachable: true, detail: "wtype found on PATH".into() };
            }
            if which("xdotool") {
                return TestResult { reachable: true, detail: "xdotool found on PATH".into() };
            }
            return TestResult {
                reachable: false,
                detail: "Neither wtype nor xdotool found".into(),
            };
        }
        #[cfg(target_os = "windows")]
        return TestResult { reachable: true, detail: "PowerShell SendKeys available".into() };
        #[allow(unreachable_code)]
        TestResult { reachable: false, detail: "Platform not supported".into() }
    }
}

// ── ClipboardTarget ───────────────────────────────────────────────────────────

pub struct ClipboardTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for ClipboardTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let mut payload = text.to_string();
        if self.0.append_newline {
            payload.push('\n');
        }
        let p = payload.clone();
        match tokio::task::spawn_blocking(move || {
            arboard::Clipboard::new()
                .context("open clipboard")?
                .set_text(&p)
                .context("set text")
        })
        .await
        {
            Ok(Ok(_)) => DeliveryResult::ok(payload),
            Ok(Err(e)) => DeliveryResult::err(e.to_string()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        let ok = tokio::task::spawn_blocking(|| arboard::Clipboard::new().is_ok())
            .await
            .unwrap_or(false);
        if ok {
            TestResult { reachable: true, detail: "Clipboard accessible".into() }
        } else {
            TestResult { reachable: false, detail: "Cannot open clipboard".into() }
        }
    }
}

// ── ExecTarget ────────────────────────────────────────────────────────────────

pub struct ExecTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for ExecTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let cmd_str = match &self.0.command {
            Some(c) => c.replace("{TEXT}", text),
            None => return DeliveryResult::err("No command configured"),
        };
        let parts: Vec<&str> = cmd_str.split_whitespace().collect();
        if parts.is_empty() {
            return DeliveryResult::err("Empty command");
        }
        match tokio::process::Command::new(parts[0])
            .args(&parts[1..])
            .spawn()
        {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        let Some(cmd) = &self.0.command else {
            return TestResult { reachable: false, detail: "No command configured".into() };
        };
        let binary = cmd.split_whitespace().next().unwrap_or("");
        if which(binary) {
            TestResult { reachable: true, detail: format!("{binary} found on PATH") }
        } else {
            TestResult { reachable: false, detail: format!("{binary} not found on PATH") }
        }
    }
}

// ── PipeTarget ────────────────────────────────────────────────────────────────

pub struct PipeTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for PipeTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(path_str) = &self.0.pipe_path else {
            return DeliveryResult::err("No pipe_path configured");
        };
        let path = shellexpand_tilde(path_str);
        if !std::path::Path::new(&path).exists() {
            return DeliveryResult::err(format!("Pipe {path} does not exist"));
        }
        let payload = format!("{text}\n").into_bytes();
        // Open FIFO for writing via std (non-blocking open)
        match std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .and_then(|mut f| { use std::io::Write; f.write_all(&payload) })
        {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        let Some(path_str) = &self.0.pipe_path else {
            return TestResult { reachable: false, detail: "No pipe_path configured".into() };
        };
        let path = shellexpand_tilde(path_str);
        let p = std::path::Path::new(&path);
        if p.exists() {
            TestResult { reachable: true, detail: format!("FIFO {path} exists") }
        } else {
            TestResult { reachable: false, detail: format!("FIFO {path} not found") }
        }
    }
}

// ── SocketTarget ──────────────────────────────────────────────────────────────

pub struct SocketTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for SocketTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let payload = format!("{text}\n").into_bytes();
        let result = if let Some(unix) = &self.0.socket_unix {
            let mut s = match UnixStream::connect(unix).await {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(e.to_string()),
            };
            s.write_all(&payload).await
        } else {
            let host = self.0.socket_host.as_deref().unwrap_or("127.0.0.1");
            let port = self.0.socket_port.unwrap_or(9000);
            let mut s = match TcpStream::connect((host, port)).await {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(e.to_string()),
            };
            s.write_all(&payload).await
        };
        match result {
            Ok(_) => DeliveryResult::ok(text.into()),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        if let Some(unix) = &self.0.socket_unix {
            match UnixStream::connect(unix).await {
                Ok(_) => TestResult { reachable: true, detail: format!("Unix socket {unix} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            }
        } else {
            let host = self.0.socket_host.as_deref().unwrap_or("127.0.0.1");
            let port = self.0.socket_port.unwrap_or(9000);
            match TcpStream::connect((host, port)).await {
                Ok(_) => TestResult { reachable: true, detail: format!("TCP {host}:{port} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            }
        }
    }
}

// ── FileTarget ────────────────────────────────────────────────────────────────

pub struct FileTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for FileTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(path_str) = &self.0.file_path else {
            return DeliveryResult::err("No file_path configured");
        };
        let path = shellexpand_tilde(path_str);
        if let Some(parent) = std::path::Path::new(&path).parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return DeliveryResult::err(e.to_string());
            }
        }
        let mut line = String::new();
        if self.0.file_timestamp {
            let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ");
            line.push_str(&format!("[{ts}] "));
        }
        line.push_str(&self.0.file_prefix);
        line.push_str(text);
        line.push('\n');

        use tokio::io::AsyncWriteExt as _;
        match tokio::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)
            .await
        {
            Ok(mut f) => match f.write_all(line.as_bytes()).await {
                Ok(_) => DeliveryResult::ok(text.into()),
                Err(e) => DeliveryResult::err(e.to_string()),
            },
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        let Some(path_str) = &self.0.file_path else {
            return TestResult { reachable: false, detail: "No file_path configured".into() };
        };
        let path = shellexpand_tilde(path_str);
        let p = std::path::Path::new(&path);
        let _parent = p.parent().unwrap_or(std::path::Path::new("."));
        TestResult {
            reachable: true,
            detail: format!(
                "{path}{}",
                if p.exists() { "" } else { " (will be created)" }
            ),
        }
    }
}

// ── DbusTarget ────────────────────────────────────────────────────────────────

pub struct DbusTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for DbusTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        #[cfg(target_os = "linux")]
        {
            let signal = self
                .0
                .dbus_signal
                .as_deref()
                .unwrap_or("ai.voxctl.Routing.TextRouted");
            match emit_dbus_signal(signal, text).await {
                Ok(_) => return DeliveryResult::ok(text.into()),
                Err(e) => return DeliveryResult::err(e.to_string()),
            }
        }
        #[cfg(not(target_os = "linux"))]
        DeliveryResult::err("DBus not available on this platform")
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        return TestResult { reachable: true, detail: "DBus available (Linux)".into() };
        #[cfg(not(target_os = "linux"))]
        TestResult { reachable: false, detail: "DBus not available on this platform".into() }
    }
}

#[cfg(target_os = "linux")]
async fn emit_dbus_signal(signal_name: &str, text: &str) -> anyhow::Result<()> {
    use zbus::Connection;
    let conn = Connection::session().await?;
    let parts: Vec<&str> = signal_name.rsplitn(2, '.').collect();
    let (member, iface) = if parts.len() == 2 {
        (parts[0], parts[1])
    } else {
        (signal_name, signal_name)
    };
    let obj_path = format!("/{}", iface.replace('.', "/"));
    conn.emit_signal(None::<&str>, obj_path.as_str(), iface, member, &(text,))
        .await?;
    Ok(())
}

// ── HttpTarget ────────────────────────────────────────────────────────────────

pub struct HttpTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for HttpTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let Some(url) = &self.0.http_url else {
            return DeliveryResult::err("No http_url configured");
        };
        let payload = build_json_payload(&self.0.http_json_template, text);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        let mut req = client.request(
            self.0.http_method.parse().unwrap_or(reqwest::Method::POST),
            url,
        );
        if let Some(headers) = &self.0.http_headers {
            for (k, v) in headers {
                if let (Ok(name), Ok(val)) = (
                    k.parse::<reqwest::header::HeaderName>(),
                    v.parse::<reqwest::header::HeaderValue>(),
                ) {
                    req = req.header(name, val);
                }
            }
        }
        req = req.json(&payload);
        match req.send().await {
            Ok(r) if r.status().is_success() => DeliveryResult::ok(text.into()),
            Ok(r) => DeliveryResult::err(format!("HTTP {}", r.status())),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        if self.0.http_url.is_some() {
            TestResult { reachable: true, detail: "HTTP target configured".into() }
        } else {
            TestResult { reachable: false, detail: "No http_url configured".into() }
        }
    }
}

// ── WebhookTarget ─────────────────────────────────────────────────────────────

pub struct WebhookTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for WebhookTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let Some(url) = &self.0.webhook_url else {
            return DeliveryResult::err("No webhook_url configured");
        };
        let Some(secret) = &self.0.webhook_secret else {
            return DeliveryResult::err("No webhook_secret configured");
        };
        let payload = build_json_payload(&self.0.webhook_json_template, text);
        let body = serde_json::to_vec(&payload).unwrap_or_default();

        let mut mac = <Hmac<Sha256>>::new_from_slice(secret.as_bytes())
            .expect("HMAC accepts any key size");
        mac.update(&body);
        let sig = hex::encode(mac.finalize().into_bytes());

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap();
        match client
            .post(url)
            .header("Content-Type", "application/json")
            .header("X-Webhook-Signature", sig)
            .body(body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => DeliveryResult::ok(text.into()),
            Ok(r) => DeliveryResult::err(format!("HTTP {}", r.status())),
            Err(e) => DeliveryResult::err(e.to_string()),
        }
    }

    async fn test(&self) -> TestResult {
        if self.0.webhook_url.is_none() {
            return TestResult { reachable: false, detail: "No webhook_url configured".into() };
        }
        if self.0.webhook_secret.is_none() {
            return TestResult { reachable: false, detail: "No webhook_secret configured".into() };
        }
        TestResult { reachable: true, detail: "Webhook target configured".into() }
    }
}

// ── McpTarget ─────────────────────────────────────────────────────────────────

pub struct McpTarget(OutputTarget);

#[async_trait::async_trait]
impl DeliveryTarget for McpTarget {
    async fn deliver(&self, text: &str) -> DeliveryResult {
        let tool = self.0.mcp_tool.as_deref().unwrap_or("speak_text");
        let args = build_json_payload(&self.0.mcp_args, text);

        #[cfg(target_os = "linux")]
        let s = {
            let path = self.0.mcp_path.as_deref().unwrap_or("/tmp/voxctl-mcp.sock");
            match UnixStream::connect(path).await {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(format!("Failed to connect to MCP socket {path}: {e}")),
            }
        };

        #[cfg(target_os = "windows")]
        let s = {
            use tokio::net::windows::named_pipe::ClientOptions;
            let path = self.0.mcp_path.as_deref().unwrap_or(r"\\.\pipe\voxctl-mcp");
            match ClientOptions::new().open(path) {
                Ok(s) => s,
                Err(e) => return DeliveryResult::err(format!("Failed to connect to MCP named pipe {path}: {e}")),
            }
        };

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        return DeliveryResult::err("MCP target not supported on this platform");

        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (reader, mut writer) = tokio::io::split(s);
        let mut lines = BufReader::new(reader).lines();

        // Step 1: initialize request
        let init_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "VoxCtr-Client",
                    "version": "1.0.0"
                }
            }
        });
        let payload = serde_json::to_string(&init_req).unwrap() + "\n";
        if let Err(e) = writer.write_all(payload.as_bytes()).await {
            return DeliveryResult::err(format!("Failed to write initialize to MCP: {e}"));
        }
        if let Err(e) = writer.flush().await {
            return DeliveryResult::err(format!("Failed to flush: {e}"));
        }

        // Read initialize response
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(err) = val.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown initialization error");
                        return DeliveryResult::err(format!("MCP initialization error: {msg}"));
                    }
                } else {
                    return DeliveryResult::err("Failed to parse JSON initialize response from MCP server");
                }
            }
            Ok(None) => return DeliveryResult::err("MCP server closed connection during initialization"),
            Err(e) => return DeliveryResult::err(format!("Failed to read initialize response from MCP server: {e}")),
        }

        // Step 2: notifications/initialized
        let initialized_notify = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let payload = serde_json::to_string(&initialized_notify).unwrap() + "\n";
        if let Err(e) = writer.write_all(payload.as_bytes()).await {
            return DeliveryResult::err(format!("Failed to write initialized notification to MCP: {e}"));
        }
        if let Err(e) = writer.flush().await {
            return DeliveryResult::err(format!("Failed to flush: {e}"));
        }

        // Step 3: tools/call
        let tool_req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "method": "tools/call",
            "params": {
                "name": tool,
                "arguments": args
            }
        });
        let payload = serde_json::to_string(&tool_req).unwrap() + "\n";
        if let Err(e) = writer.write_all(payload.as_bytes()).await {
            return DeliveryResult::err(format!("Failed to write tool call request to MCP: {e}"));
        }
        if let Err(e) = writer.flush().await {
            return DeliveryResult::err(format!("Failed to flush: {e}"));
        }

        // Read tool call response
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Ok(val) = serde_json::from_str::<serde_json::Value>(&line) {
                    if let Some(err) = val.get("error") {
                        let msg = err.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown tool call error");
                        DeliveryResult::err(format!("MCP tool call error: {msg}"))
                    } else if let Some(res) = val.get("result") {
                        DeliveryResult::ok(serde_json::to_string(res).unwrap_or_else(|_| text.to_string()))
                    } else {
                        DeliveryResult::ok(text.to_string())
                    }
                } else {
                    DeliveryResult::err("Failed to parse JSON tool call response from MCP server")
                }
            }
            Ok(None) => DeliveryResult::err("MCP server closed connection during tool call"),
            Err(e) => DeliveryResult::err(format!("Failed to read tool call response from MCP server: {e}")),
        }
    }

    async fn test(&self) -> TestResult {
        #[cfg(target_os = "linux")]
        {
            let path = self.0.mcp_path.as_deref().unwrap_or("/tmp/voxctl-mcp.sock");
            match UnixStream::connect(path).await {
                Ok(_) => TestResult { reachable: true, detail: format!("MCP socket {path} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            }
        }
        #[cfg(target_os = "windows")]
        {
            use tokio::net::windows::named_pipe::ClientOptions;
            let path = self.0.mcp_path.as_deref().unwrap_or(r"\\.\pipe\voxctl-mcp");
            match ClientOptions::new().open(path) {
                Ok(_) => TestResult { reachable: true, detail: format!("MCP named pipe {path} reachable") },
                Err(e) => TestResult { reachable: false, detail: e.to_string() },
            }
        }
        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        TestResult { reachable: false, detail: "Platform not supported".into() }
    }
}


// ── Helpers ───────────────────────────────────────────────────────────────────

fn which(bin: &str) -> bool {
    std::process::Command::new("which")
        .arg(bin)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn shellexpand_tilde(s: &str) -> String {
    if s.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            return home.join(&s[2..]).to_string_lossy().into_owned();
        }
    }
    s.to_string()
}

fn build_json_payload(
    template: &Option<serde_json::Value>,
    text: &str,
) -> serde_json::Value {
    if let Some(tmpl) = template {
        substitute_text(tmpl.clone(), text)
    } else {
        serde_json::json!({ "text": text })
    }
}

fn substitute_text(val: serde_json::Value, text: &str) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => {
            serde_json::Value::String(s.replace("{TEXT}", text))
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(|v| substitute_text(v, text)).collect())
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, substitute_text(v, text)))
                .collect(),
        ),
        other => other,
    }
}
