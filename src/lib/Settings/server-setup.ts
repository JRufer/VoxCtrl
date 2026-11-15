// What the General tab's "VoxCtrl Server" panel changes in the config, kept
// apart from the component so the rules can be tested on their own.
//
// The panel is a single place to point the app at a server and say what to
// use it for. Everything it sets is an ordinary setting that the Engine,
// Features and TTS tabs also show; there is no separate "client mode".

import type { AppConfig } from "../../stores/config";

type Backend = AppConfig["engine"]["backend"];
type TtsEngine = AppConfig["tts"]["engine"];

/** The address a VoxCtrl server listens on by default. */
export const DEFAULT_SERVER_PORT = 8787;

/** Make sure the server section exists, with nothing filled in for the user. */
export function ensureServerConfig(cfg: AppConfig) {
  if (!cfg.engine.remote_openai) {
    cfg.engine.remote_openai = {
      endpoint: "",
      api_key: null,
      model: "whisper-1",
      language: "",
      timeout_secs: 30,
      fallback: "none",
    };
  }
}

/**
 * Tidy what was typed into the address box: a bare host or host:port becomes
 * a full `/v1` address. Leaves a complete address alone, and an empty box empty.
 */
export function normalizeServerAddress(typed: string): string {
  let address = typed.trim().replace(/\/+$/, "");
  if (!address) return "";
  if (!/^https?:\/\//i.test(address)) address = `http://${address}`;
  const rest = address.replace(/^https?:\/\//i, "");
  // Just a machine name or IP address: add the server's default port.
  if (!rest.includes("/") && !/:\d+$/.test(rest) && !rest.startsWith("[")) {
    address = `${address}:${DEFAULT_SERVER_PORT}`;
  }
  if (!/\/v1(\/|$)/.test(address)) address = `${address}/v1`;
  return address;
}

/** What the server is currently used for. */
export function serverUses(cfg: AppConfig) {
  const recognition = cfg.engine.backend === "remote-openai";
  return {
    recognition,
    // Cleanup follows the recognition setting: with a local engine the app
    // cleans up locally whatever this flag says.
    cleanup: recognition && !!cfg.engine.remote_openai?.cleanup_on_server,
    speech: cfg.tts.enabled && cfg.tts.engine === "server",
  };
}

/**
 * The local engine to go back to when the server stops doing recognition:
 * the one in use before the server was switched on in this window, else the
 * fallback engine (its model is known to be wanted), else Whisper.cpp.
 */
export function localBackendToRestore(cfg: AppConfig, before: Backend | null): Exclude<Backend, "remote-openai"> {
  if (before && before !== "remote-openai") return before;
  const fallback = cfg.engine.remote_openai?.fallback;
  if (fallback && fallback !== "none") return fallback;
  return "whisper-cpp";
}

export function setRecognitionOnServer(cfg: AppConfig, on: boolean, before: Backend | null) {
  ensureServerConfig(cfg);
  cfg.engine.backend = on ? "remote-openai" : localBackendToRestore(cfg, before);
}

export function setCleanupOnServer(cfg: AppConfig, on: boolean) {
  ensureServerConfig(cfg);
  cfg.engine.remote_openai.cleanup_on_server = on;
}

/**
 * Speech on the server is the TTS engine "server"; switching it on also
 * switches text-to-speech on. Switching it off goes back to the engine in use
 * before, or to eSpeak, the one engine that needs no download.
 */
export function setSpeechOnServer(cfg: AppConfig, on: boolean, before: TtsEngine | null) {
  if (on) {
    cfg.tts.engine = "server";
    cfg.tts.enabled = true;
  } else {
    cfg.tts.engine = before && before !== "server" ? before : "espeak";
  }
}

export interface ServerReport {
  recognition: { ok: boolean; message: string; models: string[] };
  cleanup: { available: boolean; message: string };
  speech: { available: boolean; message: string };
}

/** One line for the top of the panel after a connection test. */
export function connectionSummary(report: ServerReport): { ok: boolean; text: string } {
  if (!report.recognition.ok) return { ok: false, text: report.recognition.message };
  const offers = ["speech recognition"];
  if (report.cleanup.available) offers.push("dictation cleanup");
  if (report.speech.available) offers.push("text-to-speech");
  const list = offers.length > 1 ? `${offers.slice(0, -1).join(", ")} and ${offers[offers.length - 1]}` : offers[0];
  return { ok: true, text: `Connected. This server offers ${list}.` };
}
