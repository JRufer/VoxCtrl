# Integrations

VoxCtr exposes several interfaces for external tools and services to interact with it.

---

## MCP Server

**Crate:** `crates/voxctr-mcp/`

### What is MCP?

The [Model Context Protocol](https://modelcontextprotocol.io/) is an open standard for LLM agents to call tools on local servers. VoxCtr implements an MCP server that lets AI assistants like **Claude Desktop** or **Cursor IDE** trigger voice recording and TTS.

### Transport

| Platform | Socket |
|---|---|
| Linux | `/tmp/voxctl-mcp.sock` (Unix domain socket) |
| Windows | `\\.\pipe\voxctl-mcp` (named pipe) |

### Protocol

JSON-RPC 2.0 over the socket. Follows MCP spec v2024-11-05.

**Handshake:**
```json
// Client sends:
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"claude-desktop"}}}

// Server responds:
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"voxctl","version":"1.0.0"}}}

// Client sends (notification, no id):
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

### Available Tools

#### `transcribe_voice`
Records audio and returns the transcription. Blocks until recording ends or timeout is reached.

```json
{
  "method": "tools/call",
  "params": {
    "name": "transcribe_voice",
    "arguments": { "timeout_seconds": 15 }
  }
}
```

`timeout_seconds` defaults to `mcp.record_timeout` (15.0 seconds). Returns `"(no speech detected)"` if no audio was captured.

Response:
```json
{"content": [{"type": "text", "text": "the transcribed text here"}]}
```

#### `speak_text`
Queues text for TTS playback. Returns immediately while audio plays asynchronously.

```json
{
  "method": "tools/call",
  "params": {
    "name": "speak_text",
    "arguments": { "text": "Recording started. Please speak now." }
  }
}
```

Returns: `{"content": [{"type": "text", "text": "spoken"}]}`

#### `get_status`
Returns current recording/speaking state.

```json
{"method": "tools/call", "params": {"name": "get_status", "arguments": {}}}
```

Response:
```json
{"content": [{"type": "text", "text": "{\"recording\": false, \"speaking\": false}"}]}
```

**Recommended pattern — speak then record safely:**
1. Call `speak_text` with your question
2. Poll `get_status` until `speaking = false`
3. Call `transcribe_voice`

### Enabling the MCP Server

In `config.json`:
```json
"mcp": {
  "server_enabled": true,
  "record_timeout": 15.0
}
```

### Configuring Claude Desktop

Add to `claude_desktop_config.json`:
```json
{
  "mcpServers": {
    "voxctr": {
      "command": "nc",
      "args": ["-U", "/tmp/voxctl-mcp.sock"]
    }
  }
}
```

---

## DBus Service (Linux)

**Crate:** `crates/voxctr-dbus/`

### Interface

VoxCtr registers on the session bus as:
- **Bus name:** `ai.voxctl.Dictation`
- **Object path:** `/ai/voxctl/Dictation`
- **Interface:** `ai.voxctl.Dictation`

### Methods

| Method | Signature | Description |
|---|---|---|
| `start_recording()` | `() → ()` | Begin recording |
| `stop_recording()` | `() → ()` | Stop recording and process audio |
| `toggle_recording()` | `() → ()` | Toggle recording state |
| `get_status()` | `() → s` | Returns `"idle"`, `"recording"`, or `"transcribing"` |
| `get_word_count()` | `() → u` | Total words dictated this session |

### Signals

| Signal | Signature | Description |
|---|---|---|
| `status_changed` | `(s)` | Emitted when recording state changes |
| `text_injected` | `(s)` | Emitted after text is delivered to a target |

### Example Usage

```bash
# Start recording
dbus-send --session --dest=ai.voxctl.Dictation \
  /ai/voxctl/Dictation \
  ai.voxctl.Dictation.start_recording

# Watch for injected text
dbus-monitor --session "type='signal',interface='ai.voxctl.Dictation'"

# Get current status ("idle", "recording", or "transcribing")
dbus-send --session --print-reply \
  --dest=ai.voxctl.Dictation \
  /ai/voxctl/Dictation \
  ai.voxctl.Dictation.get_status
```

The DBus service is a stub on non-Linux platforms (compiles but does nothing).

---

## Ollama Integration

**Crate:** `crates/voxctr-llm/`

### Purpose

After Whisper transcribes speech, text can optionally be rewritten by a local LLM running in [Ollama](https://ollama.ai/). Enabled per-target via `processing.ollama_enabled = true` in `targets.toml`.

### Configuration

```json
"ollama": {
  "enabled": false,
  "endpoint": "http://localhost:11434",
  "model": "llama3.2:1b",
  "mode": "clean",
  "custom_prompt": null,
  "timeout_secs": 8
}
```

### Modes

| Mode | Prompt sent to LLM |
|---|---|
| `clean` | "Fix grammar and punctuation only. Return only the corrected text, no commentary." |
| `formal` | "Rewrite in formal professional language. Return only the result." |
| `casual` | "Rewrite in casual conversational language. Return only the result." |
| `bullet` | "Convert to a bullet-point list. Return only the list." |
| `concise` | "Summarize concisely in 1-2 sentences. Return only the summary." |
| `custom` | Uses `custom_prompt` field |

### Custom Prompt

With `mode = "custom"`, the `custom_prompt` field is used as the LLM instruction:
- If `custom_prompt` contains `{text}`, that placeholder is replaced with the transcribed text
- Otherwise, the transcribed text is appended after the prompt on a new line

### Availability Caching

`OllamaClient.is_available()` probes `GET /api/tags` on first call and **caches the result**. If Ollama was unreachable at startup, it will appear unreachable until the availability cache is reset (e.g. by changing the endpoint in settings).

### Graceful Fallback

If Ollama is unreachable, the HTTP request times out, or the response cannot be parsed, VoxCtr logs the failure and delivers the **original** Whisper transcription unchanged. Text is never dropped.

### Testing the Connection

Via the Settings → Ollama tab → "Test Connection" button, or via IPC:
```typescript
const result = await invoke('test_ollama', {
  endpoint: 'http://localhost:11434',
  timeoutSecs: 5
});
// result: { success: boolean, message: string, models: string[] }
```

---

## HTTP Webhooks

### Endpoint Delivery (`http` type)
POST with JSON body to `http_url`:
```
POST https://your-endpoint.com/voice
Content-Type: application/json

{"text": "transcribed text"}
```

Configurable: `http_method`, `http_headers`, `http_json_template`.

### Signed Webhooks (`webhook` type)
Same POST (to `webhook_url`) but with HMAC-SHA256 signature:
```
X-VoxCtr-Signature: sha256=abc123...
```

---

## AT-SPI2 Context Integration (Linux)

When `atspi.context_prompt = true`, VoxCtr uses the Linux Accessibility API (AT-SPI2) to read the surrounding text from the focused text field. This text is included in the Whisper initial prompt to improve transcription continuity and vocabulary consistency.

When `atspi.auto_code_mode = true`, VoxCtr detects when the focused application is a code editor or terminal and automatically enables code-mode post-processing.

When `atspi.injection = true`, AT-SPI2 is used as the primary text injection method (before falling back to wtype/xdotool).

Requires the `at-spi2-core` package and the `org.a11y.Bus` DBus service to be running.
