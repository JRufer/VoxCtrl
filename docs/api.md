# API Reference

## Tauri IPC Commands

These are the commands the Svelte frontend (or any Tauri WebView) can call via `invoke()`.

**Source:** `src-tauri/src/commands.rs`

```typescript
import { invoke } from '@tauri-apps/api/core';
```

---

### Status & Recording

#### `get_status() → AppStatus`
Returns the current application state.

```typescript
const status = await invoke<AppStatus>('get_status');
```

```typescript
interface AppStatus {
  recording: boolean;
  processing: boolean;
  speaking: boolean;
  word_count: number;
  active_target: string;
}
```

---

#### `start_recording(targetId?: string) → void`
Begins audio capture. Optionally specifies which target to route output to.

```typescript
await invoke('start_recording', { targetId: 'default' });
```

---

#### `stop_recording() → void`
Stops audio capture and triggers the inference pipeline.

```typescript
await invoke('stop_recording');
```

---

#### `toggle_recording(targetId?: string) → void`
Starts recording if idle, stops if recording.

```typescript
await invoke('toggle_recording');
```

---

### Configuration

#### `get_config() → AppConfig`
Returns the full application configuration.

```typescript
const config = await invoke<AppConfig>('get_config');
```

---

#### `save_config(config: AppConfig) → void`
Persists the configuration to `~/.config/voxctl/config.json`.

```typescript
await invoke('save_config', { config: myConfig });
```

---

### Routing

#### `get_targets() → OutputTarget[]`
Returns all output targets from `targets.toml`.

```typescript
const targets = await invoke<OutputTarget[]>('get_targets');
```

---

#### `save_targets(targets: OutputTarget[]) → void`
Writes updated targets to `targets.toml` and hot-reloads the router.

```typescript
await invoke('save_targets', { targets: myTargets });
```

---

#### `get_bindings() → HotkeyBinding[]`
Returns all hotkey bindings from `bindings.toml`.

```typescript
const bindings = await invoke<HotkeyBinding[]>('get_bindings');
```

---

#### `save_bindings(bindings: HotkeyBinding[]) → void`
Writes updated bindings to `bindings.toml` and hot-reloads the hotkey listener.

```typescript
await invoke('save_bindings', { bindings: myBindings });
```

---

### History

#### `get_history() → HistoryEntry[]`
Returns the transcription history (in-memory, current session only).

```typescript
const history = await invoke<HistoryEntry[]>('get_history');
```

```typescript
interface HistoryEntry {
  id: number;
  text: string;
  raw_text: string;
  target_id: string;
  timestamp: string;    // ISO 8601
  word_count: number;
  inference_ms: number;
  language: string;
}
```

---

#### `clear_history() → void`
Clears the in-memory history log.

```typescript
await invoke('clear_history');
```

---

### Text-to-Speech

#### `speak_text(text: string, voice?: string) → void`
Queues text for TTS playback.

```typescript
await invoke('speak_text', { text: 'Hello world', voice: 'en_US-lessac-medium' });
```

---

#### `check_voice_downloaded(name: string) → boolean`
Returns whether a Piper voice pack is available locally.

```typescript
const downloaded = await invoke<boolean>('check_voice_downloaded', {
  name: 'en_US-lessac-medium'
});
```

---

#### `download_voice(name: string) → void`
Downloads a Piper voice pack from GitHub. Progress is emitted via Tauri events.

```typescript
await invoke('download_voice', { name: 'en_US-ryan-high' });
```

---

### Speech Recognition Models

#### `check_model_downloaded(size: string) → boolean`
Returns whether a Whisper model file is present locally.

```typescript
const downloaded = await invoke<boolean>('check_model_downloaded', { size: 'base' });
```

---

#### `download_model(size: string) → void`
Downloads a Whisper GGUF model. Size: `"tiny"` | `"base"` | `"small"` | `"medium"` | `"large-v3"`.

```typescript
await invoke('download_model', { size: 'small' });
```

---

### Audio Monitoring

#### `start_monitoring_audio() → void`
Starts streaming `audio-level` events to the frontend (for VU meter display).

```typescript
await invoke('start_monitoring_audio');
```

---

#### `stop_monitoring_audio() → void`
Stops `audio-level` event streaming.

```typescript
await invoke('stop_monitoring_audio');
```

---

#### `list_audio_devices() → AudioDevice[]`
Returns all available input devices.

```typescript
const devices = await invoke<AudioDevice[]>('list_audio_devices');
```

```typescript
interface AudioDevice {
  index: number;
  name: string;
}
```

---

### Ollama

#### `test_ollama(endpoint: string, timeoutSecs: number) → OllamaTestResult`
Pings an Ollama endpoint and lists available models.

```typescript
const result = await invoke<OllamaTestResult>('test_ollama', {
  endpoint: 'http://localhost:11434',
  timeoutSecs: 5
});
```

```typescript
interface OllamaTestResult {
  success: boolean;
  message: string;
  models: string[];
}
```

---

### Overlay

#### `show_overlay() → void`
Makes the overlay window visible.

```typescript
await invoke('show_overlay');
```

---

#### `hide_overlay() → void`
Hides the overlay window.

```typescript
await invoke('hide_overlay');
```

---

## Tauri Events (Backend → Frontend)

Subscribe with `listen()` from `@tauri-apps/api/event`.

```typescript
import { listen } from '@tauri-apps/api/event';
```

---

### `status-tick`
Emitted every 250ms with the current application state.

```typescript
await listen<AppStatus>('status-tick', (event) => {
  console.log(event.payload.recording); // boolean
});
```

---

### `config-changed`
Emitted when the config file is modified externally (file watcher) or by another window.

```typescript
await listen<AppConfig>('config-changed', (event) => {
  config.set(event.payload);
});
```

---

### `audio-level`
Emitted during audio monitoring with the current RMS energy level (0.0 – 1.0).

```typescript
await listen<number>('audio-level', (event) => {
  updateVuMeter(event.payload);
});
```

---

## TypeScript Types

```typescript
interface AppConfig {
  engine: EngineConfig;
  audio: AudioConfig;
  ui: UiConfig;
  features: FeaturesConfig;
  ollama: OllamaConfig;
  tts: TtsConfig;
  mcp: McpConfig;
  atspi: AtspiConfig;
}

interface EngineConfig {
  backend: 'whisper' | 'moonshine';
  model_size: 'tiny' | 'base' | 'small' | 'medium' | 'large-v3';
  device: 'auto' | 'cpu' | 'cuda' | 'vulkan';
  language: string;
}

interface AudioConfig {
  device_index: number;
  gain: number;
  vad_threshold: number;
  dynamic_stream: boolean;
}

interface UiConfig {
  overlay_style: 'ocean_wave' | 'voice_card' | 'waveform' | 'pulse';
  auto_show_overlay: boolean;
  show_notification: boolean;
  history_enabled: boolean;
}

interface FeaturesConfig {
  remove_fillers: boolean;
  snippets: Record<string, string>;
  vocabulary: string[];
}

interface OllamaConfig {
  enabled: boolean;
  endpoint: string;
  model: string;
  mode: 'clean' | 'formal' | 'casual' | 'bullet' | 'concise' | 'custom';
  custom_prompt: string;
}

interface TtsConfig {
  enabled: boolean;
  engine: 'piper' | 'espeak';
  voice: string;
  stop_keys: string[];
}

interface McpConfig {
  enabled: boolean;
  record_timeout: number;
}

interface AtspiConfig {
  enabled: boolean;
  context_prompt: boolean;
  auto_code_mode: boolean;
}

interface OutputTarget {
  id: string;
  label: string;
  delivery: 'inject' | 'clipboard' | 'exec' | 'pipe' | 'socket' | 'file' | 'dbus' | 'http' | 'webhook' | 'mcp';
  file_path?: string;
  file_prefix: string;
  file_timestamp: boolean;
  http_url?: string;
  webhook_secret?: string;
  exec_command?: string;
  socket_path?: string;
  pipe_path?: string;
  send_on_release: boolean;
  append_newline: boolean;
  initial_prompt?: string;
  tts_engine: string;
  processing?: TargetProcessingConfig;
}

interface HotkeyBinding {
  id: string;
  label: string;
  keys: string[];
  gesture: 'hold' | 'toggle' | 'double';
  target_id?: string;
  target_ids: string[];
  disabled: boolean;
}
```
