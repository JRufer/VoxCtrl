// The rules behind the Engine tab's Remote Speech Engine section, where a
// server is set up and what to use it for is chosen. Kept apart from the
// component so they can be tested on their own.

import type { AppConfig } from "../../stores/config";

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
export function normalizeServerAddress(typed: string, encrypted = false): string {
  let address = typed.trim().replace(/\/+$/, "");
  if (!address) return "";
  // Only what was typed without "http://" is treated as a VoxCtrl server on
  // the network and given its default port. An address typed in full may be
  // any OpenAI-compatible service and keeps the port it was given.
  if (!/^https?:\/\//i.test(address)) {
    const bare = !address.includes("/") && !/:\d+$/.test(address) && !address.startsWith("[");
    // A paired server is only ever spoken to over https.
    address = `${encrypted ? "https" : "http"}://${address}${bare ? `:${DEFAULT_SERVER_PORT}` : ""}`;
  }
  const path = address.replace(/^https?:\/\/[^/]*/i, "");
  if (!path) address = `${address}/v1`;
  return address;
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
  llm: { available: boolean; message: string };
}

/** One line for the top of the panel after a connection test. */
export function connectionSummary(report: ServerReport): { ok: boolean; text: string } {
  if (!report.recognition.ok) return { ok: false, text: report.recognition.message };
  const offers = ["speech recognition"];
  if (report.cleanup.available) offers.push("dictation cleanup");
  if (report.speech.available) offers.push("text-to-speech");
  if (report.llm.available) offers.push("AI processing");
  const list = offers.length > 1 ? `${offers.slice(0, -1).join(", ")} and ${offers[offers.length - 1]}` : offers[0];
  return { ok: true, text: `Connected. This server offers ${list}.` };
}

/** What the server's pairing code says, as `read_pairing_code` returns it. */
export interface PairingDetails {
  endpoint: string;
  api_key: string | null;
  tls_fingerprint: string | null;
}

/** Take the address, key and certificate from a pairing code. */
export function applyPairing(cfg: AppConfig, details: PairingDetails) {
  const remote = cfg.engine.remote_openai;
  remote.endpoint = details.endpoint;
  remote.api_key = details.api_key;
  remote.tls_fingerprint = details.tls_fingerprint;
}

/** A fingerprint as people compare them: `AB:CD:…`, shortened to both ends. */
export function shortFingerprint(fingerprint: string): string {
  const pairs = fingerprint.toUpperCase().match(/.{1,2}/g) ?? [];
  if (pairs.length <= 8) return pairs.join(":");
  return `${pairs.slice(0, 4).join(":")} … ${pairs.slice(-4).join(":")}`;
}

/** Whether text-to-speech currently goes through the server. */
export function speechOnServer(cfg: AppConfig): boolean {
  return !!cfg.tts?.enabled && cfg.tts.engine === "server";
}
