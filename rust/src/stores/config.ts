import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

export interface AppConfig {
  engine: EngineConfig;
  audio: AudioConfig;
  ui: UiConfig;
  features: FeaturesConfig;
  ollama: OllamaConfig;
  tts: TtsConfig;
  mcp: McpConfig;
  atspi: AtspiConfig;
}

export interface EngineConfig {
  backend: "auto" | "whisper-cpp" | "moonshine";
  inference_mode: "Balanced" | "Aggressive";
  whisper_cpp: WhisperCppConfig;
  moonshine: MoonshineConfig;
}

export interface WhisperCppConfig {
  model_dir: string;
  model_size: string;
  device: string;
  threads: number;
}

export interface MoonshineConfig {
  model_size: string;
  language: string;
}

export interface AudioConfig {
  vad_threshold: number;
  min_silence_duration_ms: number;
  input_device_index: number | null;
  evdev_device: string | null;
  noise_suppression: boolean;
  gain: number;
}

export interface UiConfig {
  show_overlay: boolean;
  overlay_style: "voice_card" | "waveform" | "pulse" | "blue_wave" | "none";
}

export interface FeaturesConfig {
  remove_fillers: boolean;
  custom_vocabulary: string[];
  spoken_punctuation: boolean;
  auto_format_lists: boolean;
  quiet_mode: boolean;
  show_notification: boolean;
  snippets: Record<string, string>;
}

export interface OllamaConfig {
  enabled: boolean;
  model: string;
  mode: "clean" | "formal" | "casual" | "bullet" | "concise" | "custom";
  custom_prompt: string | null;
  endpoint: string;
  timeout_secs: number;
}

export interface TtsConfig {
  enabled: boolean;
  engine: "piper" | "espeak";
  voice: string;
  stop_key: string[];
  response_overlay: boolean;
}

export interface McpConfig {
  server_enabled: boolean;
  record_timeout: number;
}

export interface AtspiConfig {
  injection: boolean;
  context_prompt: boolean;
  auto_code_mode: boolean;
}

const defaultConfig: AppConfig = {
  engine: {
    backend: "auto",
    inference_mode: "Balanced",
    whisper_cpp: {
      model_dir: "",
      model_size: "large-v3",
      device: "auto",
      threads: 0,
    },
    moonshine: { model_size: "base", language: "en" },
  },
  audio: {
    vad_threshold: 0.5,
    min_silence_duration_ms: 500,
    input_device_index: null,
    evdev_device: null,
    noise_suppression: false,
    gain: 1.0,
  },
  ui: { show_overlay: true, overlay_style: "voice_card" },
  features: {
    remove_fillers: true,
    custom_vocabulary: [],
    spoken_punctuation: true,
    auto_format_lists: true,
    quiet_mode: false,
    show_notification: false,
    snippets: {},
  },
  ollama: {
    enabled: false,
    model: "llama3.2:1b",
    mode: "clean",
    custom_prompt: null,
    endpoint: "http://localhost:11434",
    timeout_secs: 8,
  },
  tts: {
    enabled: false,
    engine: "piper",
    voice: "en-us-lessac-medium",
    stop_key: ["KEY_ESCAPE"],
    response_overlay: true,
  },
  mcp: { server_enabled: false, record_timeout: 15.0 },
  atspi: { injection: true, context_prompt: true, auto_code_mode: true },
};

export const config = writable<AppConfig>(defaultConfig);
export const configDirty = writable(false);

export async function loadConfig() {
  try {
    const loaded = await invoke<AppConfig>("get_config");
    config.set(loaded);
    configDirty.set(false);
  } catch (e) {
    console.error("loadConfig:", e);
  }
}

export async function saveConfig(cfg: AppConfig) {
  await invoke("save_config", { newConfig: cfg });
  configDirty.set(false);
}

loadConfig();
