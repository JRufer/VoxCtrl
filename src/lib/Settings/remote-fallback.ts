// The fallback engine behind a remote speech server, and whether that server
// can currently be reached. Kept apart from EngineTab.svelte so the wording
// and the option list can be tested without rendering the tab.

import type { EngineConfig } from "../../stores/config";

/** `engine.remote_openai.fallback`: which on-device engine takes over. */
export type RemoteFallback = "none" | "whisper-cpp" | "moonshine" | "parakeet";

/** The payload of the `get_remote_status` command and `remote-status` event. */
export interface RemoteStatus {
  state: "not_in_use" | "unknown" | "online" | "offline";
  /** Why the server counts as offline. */
  reason: string | null;
  /** The saved fallback engine and model, e.g. "Parakeet tdt-0.6b-v3". */
  fallback: string | null;
}

/** The model a fallback choice would run: the one set in that engine's own section. */
export function fallbackModel(engine: EngineConfig, choice: RemoteFallback | undefined): string | null {
  switch (choice) {
    case "whisper-cpp":
      return engine.whisper_cpp?.model_size ?? null;
    case "moonshine":
      return engine.moonshine?.model_size ?? null;
    case "parakeet":
      return engine.parakeet?.model_size ?? null;
    default:
      return null;
  }
}

/**
 * The choices for the fallback dropdown. An engine this build was compiled
 * without is left out: selecting it would quietly run whisper.cpp instead.
 */
export function fallbackOptions(
  engine: EngineConfig,
  available: { moonshine: boolean; parakeet: boolean },
): { value: RemoteFallback; label: string }[] {
  const options: { value: RemoteFallback; label: string }[] = [
    { value: "none", label: "None — dictation fails with an error" },
    { value: "whisper-cpp", label: `Whisper.cpp (${fallbackModel(engine, "whisper-cpp")})` },
  ];
  if (available.moonshine) {
    options.push({ value: "moonshine", label: `Moonshine (${fallbackModel(engine, "moonshine")})` });
  }
  if (available.parakeet) {
    options.push({ value: "parakeet", label: `Parakeet TDT (${fallbackModel(engine, "parakeet")})` });
  }
  return options;
}

/**
 * One line on whether the server is reachable and what that means for the
 * next dictation. `null` when there is nothing to say.
 */
export function statusSummary(status: RemoteStatus | null): { tone: "ok" | "warn" | "muted"; text: string } | null {
  if (!status) return null;
  switch (status.state) {
    case "online":
      return { tone: "ok", text: "Connected to the server." };
    case "unknown":
      return { tone: "muted", text: "Checking the server…" };
    case "offline": {
      const reason = status.reason?.trim() || "The server could not be reached";
      const next = status.fallback
        ? `Dictation is using the fallback engine (${status.fallback}).`
        : "Dictation will fail until it is back.";
      return { tone: "warn", text: `Server unreachable: ${reason}. ${next}` };
    }
    default:
      // The saved backend is a local one: a remote engine picked here has not
      // been saved yet, so nothing is watching the server.
      return { tone: "muted", text: "Save your settings to start checking the server." };
  }
}
