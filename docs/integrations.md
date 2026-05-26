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

The socket is owned by the current user with mode `0600` (no other user can connect).

### Protocol

JSON-RPC 2.0 over the socket. Follows MCP spec v2024-11-05.

**Handshake:**
```json
// Client sends:
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{"name":"claude-desktop"}}}

// Server responds:
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"voxctr","version":"..."}}}

// Client sends:
{"jsonrpc":"2.0","method":"notifications/initialized"}
```

### Available Tools

#### `transcribe_voice`
Records audio and returns the transcription as text.

```json
{
  "method": "tools/call",
  "params": {
    "name": "transcribe_voice",
    "arguments": {
      "timeout_seconds": 30
    }
  }
}
```

Response:
```json
{
  "result": {
    "content": [{"type": "text", "text": "the transcribed text here"}]
  }
}
```

The call blocks until recording completes (hotkey released) or the timeout is reached.

#### `speak_text`
Queues text for TTS playback.

```json
{
  "method": "tools/call",
  "params": {
    "name": "speak_text",
    "arguments": {
      "text": "Recording started. Please speak now."
    }
  }
}
```

#### `get_status`
Returns current recording/speaking state.

```json
{"method": "tools/call", "params": {"name": "get_status", "arguments": {}}}
```

Response:
```json
{"content": [{"type": "text", "text": "{\"recording\": false, \"speaking\": false}"}]}
```

### Configuring Claude Desktop

Add to your Claude Desktop `claude_desktop_config.json`:
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

Or use a proxy script that connects to the socket.

### Enabling the MCP Server

In `config.json`:
```json
"mcp": {
  "enabled": true,
  "record_timeout": 30
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
| `get_status()` | `() → s` | Returns `"recording"`, `"processing"`, or `"idle"` |
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

# Get current status
dbus-send --session --print-reply \
  --dest=ai.voxctl.Dictation \
  /ai/voxctl/Dictation \
  ai.voxctl.Dictation.get_status
```

### Use Cases
- Desktop environment macros
- i3/Sway workspace scripts
- System-wide voice command pipeline

---

## Ollama Integration

**Crate:** `crates/voxctr-llm/`

### Purpose

After Whisper transcribes your speech, the raw text can optionally be rewritten by a local LLM running in [Ollama](https://ollama.ai/). This is useful for correcting grammar, adjusting tone, or reformatting content.

### Configuration

```json
"ollama": {
  "enabled": true,
  "endpoint": "http://localhost:11434",
  "model": "llama3.2",
  "mode": "clean",
  "custom_prompt": ""
}
```

### Modes

| Mode | Behavior |
|---|---|
| `clean` | Fix grammar, punctuation, and capitalization |
| `formal` | Rewrite in formal/professional language |
| `casual` | Rewrite in conversational language |
| `bullet` | Convert to a bullet-point list |
| `concise` | Shorten and summarize |
| `custom` | Use `custom_prompt` as the instruction |

### Custom Prompt Template

With `mode = "custom"`, the `custom_prompt` field is used as the LLM instruction. The transcribed text is appended:

```json
"custom_prompt": "Rewrite the following as a professional email reply:"
```

### Graceful Fallback

If Ollama is unreachable or returns an error, VoxCtr logs the failure and delivers the **original** Whisper transcription unchanged. It never blocks or drops text.

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
POST with JSON body:
```
POST https://your-endpoint.com/voice
Content-Type: application/json

{"text": "transcribed text"}
```

### Signed Webhooks (`webhook` type)
Same POST but with additional HMAC-SHA256 signature:
```
X-VoxCtr-Signature: sha256=abc123...
```

Verify on your server:
```python
import hmac, hashlib

def verify(payload: bytes, secret: str, signature: str) -> bool:
    expected = 'sha256=' + hmac.new(
        secret.encode(), payload, hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, signature)
```

---

## AT-SPI2 Context Integration (Linux)

When `atspi.enabled = true`, VoxCtr uses the Linux Accessibility API (AT-SPI2) to:

1. **Read focused widget text** — Retrieves the last ~200 characters from the focused text field to use as a Whisper context prompt, improving transcription continuity.
2. **Auto code mode** — Detects if the focused application is a code editor and automatically enables code-mode post-processing.

Requires the `at-spi2-core` package and the `org.a11y.Bus` DBus service.
