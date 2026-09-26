/**
 * Static tables the setup wizard presents, plus the small formatting helpers
 * its cards share.
 *
 * The wizard deliberately offers a *subset* of what Settings exposes. A first
 * launch is the wrong moment to show eleven Whisper variants or every Piper
 * voice: the point is to get to working dictation, and every extra option is
 * one more decision made with no experience of the app. Everything omitted here
 * is still reachable in Settings afterwards.
 */

export type SttEngineId = "whisper-cpp" | "moonshine" | "parakeet" | "remote-openai";

export interface ModelOption {
  /** Value written into the config (`whisper_cpp.model_size` / `moonshine.model_size` / `parakeet.model_size`). */
  id: string;
  /** Approximate download size in MB, for the size label. */
  mb: number;
  /** 0-1, relative transcription speed in a fresh CPU-only run. */
  speed: number;
  /** 0-1, relative accuracy in a quiet room. */
  accuracy: number;
}

/** The five Whisper sizes worth choosing between on a first run. */
export const WHISPER_MODELS: ModelOption[] = [
  { id: "tiny", mb: 75, speed: 0.96, accuracy: 0.58 },
  { id: "base", mb: 142, speed: 0.88, accuracy: 0.68 },
  { id: "small", mb: 466, speed: 0.68, accuracy: 0.8 },
  { id: "medium", mb: 1500, speed: 0.42, accuracy: 0.9 },
  { id: "large-v3", mb: 3100, speed: 0.22, accuracy: 0.97 },
];

/** Moonshine ships exactly two sizes. */
export const MOONSHINE_MODELS: ModelOption[] = [
  { id: "tiny", mb: 100, speed: 0.98, accuracy: 0.64 },
  { id: "base", mb: 250, speed: 0.92, accuracy: 0.77 },
];

/** NVIDIA Parakeet TDT 0.6B v3. */
export const PARAKEET_MODELS: ModelOption[] = [
  { id: "tdt-0.6b-v3", mb: 665, speed: 0.95, accuracy: 0.94 },
];

/** Remote OpenAI-compatible Speech Engine (offloaded, network-based). */
export const REMOTE_OPENAI_MODELS: ModelOption[] = [
  { id: "remote", mb: 0, speed: 0.95, accuracy: 0.95 },
];

export interface SttEngineInfo {
  id: SttEngineId;
  name: string;
  glyph: string;
  tagline: string;
  models: ModelOption[];
  /** Whether GPU offloading applies to this engine at all. */
  gpu: boolean;
  /** Accuracy retained in a noisy room, as a fraction of the quiet-room figure. */
  noiseRetention: number;
}

export const STT_ENGINES: SttEngineInfo[] = [
  {
    id: "whisper-cpp",
    name: "whisper.cpp",
    glyph: "∇",
    tagline:
      "Reference-grade accuracy in quiet spaces. Five model sizes from 75 MB to 3.1 GB. CUDA, Vulkan or CPU.",
    models: WHISPER_MODELS,
    gpu: true,
    noiseRetention: 0.64,
  },
  {
    id: "moonshine",
    name: "Moonshine",
    glyph: "↯",
    tagline:
      "Streaming ONNX model tuned for real rooms — fans, keyboards, traffic. Fast on any CPU, tiny download.",
    models: MOONSHINE_MODELS,
    gpu: false,
    noiseRetention: 0.93,
  },
  {
    id: "parakeet",
    name: "Parakeet TDT",
    glyph: "⚡",
    tagline:
      "NVIDIA FastConformer TDT 0.6B. Ultra-fast non-autoregressive transcription, SOTA accuracy, no repetition loops.",
    models: PARAKEET_MODELS,
    gpu: true,
    noiseRetention: 0.88,
  },
  {
    id: "remote-openai",
    name: "Remote Speech Engine",
    glyph: "🌐",
    tagline:
      "Connect to an OpenAI-compatible speech-to-text server on your network or cloud (Faster-Whisper, vLLM, etc.).",
    models: REMOTE_OPENAI_MODELS,
    gpu: false,
    noiseRetention: 0.92,
  },
];

export type GestureId = "toggle" | "double_tap" | "hold" | "double_tap_hold";

export interface GestureInfo {
  id: GestureId;
  name: string;
  /** How the Test step tells the user to trigger the binding. */
  verb: string;
  desc: string;
  /** [flex, on] segments drawn in the key timeline. */
  key: [number, number][];
  /** [flex, on] segments drawn in the mic timeline. */
  mic: [number, number][];
}

export const GESTURES: GestureInfo[] = [
  {
    id: "toggle",
    name: "Tap to talk",
    verb: "tap",
    desc: "Press once to start, press again to stop. Hands stay free while you speak.",
    key: [[1, 1], [8, 0], [1, 1], [3, 0]],
    mic: [[1, 0], [9, 1], [4, 0]],
  },
  {
    id: "double_tap",
    name: "Double-tap to talk",
    verb: "double-tap",
    desc: "Two quick presses start dictation; one press stops. Hard to trigger by accident.",
    key: [[1, 1], [0.7, 0], [1, 1], [7, 0], [1, 1], [3, 0]],
    mic: [[2.7, 0], [7.3, 1], [4, 0]],
  },
  {
    id: "hold",
    name: "Hold to talk",
    verb: "hold",
    desc: "Hold the keys down while speaking, release to stop. Like a walkie-talkie.",
    key: [[1, 0], [9, 1], [4, 0]],
    mic: [[1, 0], [9, 1], [4, 0]],
  },
  {
    id: "double_tap_hold",
    name: "Double-tap & hold",
    verb: "double-tap and hold",
    desc: "Tap once, then press and hold. Release to stop. Never collides with a plain shortcut.",
    key: [[1, 1], [0.7, 0], [8.3, 1], [4, 0]],
    mic: [[2.7, 0], [6.3, 1], [4, 0]],
  },
];

export interface OverlayInfo {
  /** Value written into `ui.overlay_style`. */
  id: string;
  name: string;
  meta: string;
  glyph: string;
}

/** Mirrors the built-in list in Settings → Visual. Custom overlays are not
 *  offered here: a machine on its first launch has none. */
export const OVERLAY_STYLES: OverlayInfo[] = [
  { id: "blue_wave", name: "Ocean Wave", meta: "default · tide-pool waves", glyph: "⊟" },
  { id: "voice_card", name: "Voice Card", meta: "20×6 LED VU matrix", glyph: "⊟" },
  { id: "waveform", name: "Waveform", meta: "scrolling oscilloscope", glyph: "⌇" },
  { id: "pulse", name: "Pulse Ring", meta: "sonar sweep · target lock", glyph: "◌" },
  { id: "mono_bars", name: "Mono Bars", meta: "black & white 5-bar meter", glyph: "▥" },
  { id: "spectrum", name: "Neon Spectrum", meta: "16-band equalizer", glyph: "▦" },
  { id: "terminal", name: "Retro Terminal", meta: "DOS-blue ASCII meter", glyph: "▸" },
  { id: "vinyl", name: "Analog VU", meta: "vintage needle meter", glyph: "◔" },
];

export const OVERLAY_POSITIONS: { id: string; label: string; glyph: string }[] = [
  { id: "top", label: "Top", glyph: "⤒" },
  { id: "center", label: "Center", glyph: "⊡" },
  { id: "bottom", label: "Bottom", glyph: "⤓" },
];

export type TtsEngineId =
  | "vox_cpm_2"
  | "breeze_tts_2"
  | "pocket_tts"
  | "piper"
  | "inflect_micro"
  | "espeak";

export interface TtsEngineInfo {
  id: TtsEngineId;
  name: string;
  kind: string;
  /** 0-1 relative voice quality. */
  quality: number;
  /** 0-1 relative synthesis speed. */
  speed: number;
  /** Approximate download in MB; 0 means nothing to fetch. */
  mb: number;
  note: string;
  /**
   * Whether the weights sit behind a gated HuggingFace repo, so nothing can be
   * downloaded — and the engine cannot be picked — until an access token is
   * entered.
   */
  needsHfToken?: boolean;
  /** Where the user accepts this model's licence, shown alongside the token field. */
  licenceUrl?: string;
  /**
   * Weights carry a non-commercial license — surfaced as a warning before the
   * user downloads them, distinct from `needsHfToken` (a gate on *access*,
   * not on *use*).
   */
  nonCommercial?: boolean;
}

export const TTS_ENGINES: TtsEngineInfo[] = [
  {
    id: "vox_cpm_2",
    name: "VoxCPM2",
    kind: "neural · 2B autoregressive",
    quality: 0.98,
    speed: 0.25,
    mb: 2950,
    note: "2B autoregressive diffusion. SOTA voice realism & cloning. Runs via audio.cpp, best on a Vulkan GPU.",
  },
  {
    id: "breeze_tts_2",
    name: "Breeze-TTS-2",
    kind: "neural · expressive",
    quality: 0.96,
    speed: 0.32,
    mb: 5080,
    note: "Most natural prosody. Runs via audio.cpp, best on a Vulkan GPU or fast CPU.",
    nonCommercial: true,
    licenceUrl: "huggingface.co/BreezeBlue/Breeze-TTS-2",
  },
  {
    id: "pocket_tts",
    name: "Pocket TTS",
    kind: "neural · voice cloning",
    quality: 0.86,
    speed: 0.55,
    mb: 130,
    note: "Cloned voices, downloaded in-app. Runs via audio.cpp with optional Vulkan acceleration.",
  },
  {
    id: "piper",
    name: "Piper TTS",
    kind: "onnx · ~11 voices",
    quality: 0.72,
    speed: 0.8,
    mb: 60,
    note: "The sweet spot for most machines. Per-voice ~60 MB.",
  },
  {
    id: "inflect_micro",
    name: "Inflect Micro",
    kind: "compact neural",
    quality: 0.55,
    speed: 0.92,
    mb: 38,
    note: "Tiny footprint, still human-ish. Good for laptops.",
  },
  {
    id: "espeak",
    name: "eSpeak-NG",
    kind: "formant synth",
    quality: 0.3,
    speed: 0.99,
    mb: 0,
    note: "Instant, robotic, runs anywhere. Zero download.",
  },
];

export const STEP_LABELS = [
  "Welcome",
  "Engine",
  "Hotkey",
  "Overlay",
  "Test",
  "Voice",
  "Done",
] as const;

// ── Formatting helpers ───────────────────────────────────────────────────────

/** "466 MB" / "1.5 GB", so a size never reads as a bare four-digit number. */
export function formatSize(mb: number): string {
  if (mb <= 0) return "none";
  return mb >= 1000 ? `${(mb / 1000).toFixed(1)} GB` : `${Math.round(mb)} MB`;
}

/**
 * How full a model-size bar should be, as a percentage of the largest model on
 * offer.
 *
 * Straight proportion, deliberately. This started life on a log scale, which
 * made 60 MB and 1.2 GB look like neighbours — Piper read as half the download
 * of Breeze-TTS-2 when it is one twentieth of it, which is exactly the
 * comparison the row exists to make. A near-empty bar next to a full one is
 * the honest picture, and the size is printed beside it either way.
 */
export function modelSizeShare(mb: number, engines: { mb: number }[] = TTS_ENGINES): number {
  const largest = Math.max(0, ...engines.map((e) => e.mb));
  if (largest <= 0 || mb <= 0) return 0;
  return Math.min(100, (mb / largest) * 100);
}

export function formatPercent(v: number): string {
  return `${Math.round(v * 100)}%`;
}

/** Turn a 0-1 speed score into the phrase a user can act on. */
export function speedLabel(v: number): string {
  if (v > 0.85) return "instant";
  if (v > 0.6) return "< 1 s";
  if (v > 0.35) return "1–3 s";
  return "3–8 s";
}

export function ttsSpeedLabel(v: number): string {
  if (v > 0.9) return "instant";
  if (v > 0.7) return "< 0.5 s";
  if (v > 0.5) return "~1 s";
  return "2–4 s";
}

/**
 * A deterministic bar profile for the accuracy sparklines. Seeded off the
 * value itself so the same model always draws the same shape — a preview that
 * reshuffled on every render would read as live data, which it is not.
 */
export function accuracyBars(count: number, value: number): { h: number; o: number }[] {
  return Array.from({ length: count }, (_, i) => {
    const t = (i + 1) / count;
    return {
      h: Math.max(8, Math.round(100 * value * (0.55 + 0.45 * Math.sin(i * 1.7 + value * 5)))),
      o: t <= value ? 1 : 0.18,
    };
  });
}

/** Animation offsets for a row of "sound is happening" bars. */
export function waveBars(count: number): { d: string; dl: string }[] {
  return Array.from({ length: count }, (_, i) => ({
    d: (0.6 + ((i * 7) % 5) * 0.12).toFixed(2),
    dl: (i * 0.07).toFixed(2),
  }));
}

// ── Key naming ───────────────────────────────────────────────────────────────


const KEYCAP_LABELS: Record<string, string> = {
  KEY_LEFTCTRL: "Ctrl",
  KEY_RIGHTCTRL: "Ctrl",
  KEY_LEFTALT: "Alt",
  KEY_RIGHTALT: "Alt",
  KEY_LEFTSHIFT: "Shift",
  KEY_RIGHTSHIFT: "Shift",
  KEY_LEFTMETA: "Super",
  KEY_RIGHTMETA: "Super",
  KEY_SPACE: "Space",
  KEY_ESC: "Esc",
  KEY_ENTER: "Enter",
  KEY_TAB: "Tab",
  KEY_BACKSPACE: "Backspace",
  KEY_DELETE: "Delete",
  KEY_MINUS: "-",
  KEY_EQUAL: "=",
  KEY_LEFTBRACE: "[",
  KEY_RIGHTBRACE: "]",
  KEY_BACKSLASH: "\\",
  KEY_SEMICOLON: ";",
  KEY_APOSTROPHE: "'",
  KEY_GRAVE: "`",
  KEY_COMMA: ",",
  KEY_DOT: ".",
  KEY_SLASH: "/",
  KEY_KPPLUS: "Num +",
  KEY_KPMINUS: "Num -",
  KEY_KPASTERISK: "Num *",
  KEY_KPSLASH: "Num /",
  KEY_KPDOT: "Num .",
  KEY_KPENTER: "Num Enter",
  KEY_KPEQUAL: "Num =",
};

/** Human label for an evdev key name, for keycaps and summaries. */
export function keycapLabel(evdev: string): string {
  const kpDigit = /^KEY_KP(\d)$/.exec(evdev);
  if (kpDigit) return `Num ${kpDigit[1]}`;
  return KEYCAP_LABELS[evdev] ?? evdev.replace(/^KEY_/, "");
}

// Shared with the Settings recorders; see src/lib/keys.ts.
export { MODIFIER_KEYS, isModifiersOnly, mapBrowserKeyToEvdev } from "../keys";
