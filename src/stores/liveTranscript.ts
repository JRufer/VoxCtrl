import { writable, derived } from "svelte/store";
import { config } from "./config";
import { status } from "./status";

/**
 * The words being transcribed while the user is still speaking.
 *
 * `committed` words have been agreed on by consecutive transcription passes
 * and will not change; `tentative` words are the engine's current guess at
 * the most recent speech and may still be corrected. `final` is set once the
 * finished (and cleaned-up) transcript replaces the live guess.
 */
export interface LiveTranscript {
  session_id: number;
  committed: string;
  tentative: string;
  final: boolean;
}

export const EMPTY_LIVE_TRANSCRIPT: LiveTranscript = {
  session_id: 0,
  committed: "",
  tentative: "",
  final: false,
};

export const liveTranscript = writable<LiveTranscript>({ ...EMPTY_LIVE_TRANSCRIPT });

/** The whole transcript as one string: committed words, then tentative ones. */
export function liveFullText(t: LiveTranscript): string {
  return [t.committed, t.tentative].filter((s) => s.trim() !== "").join(" ");
}

/**
 * Whether the built-in overlays should show their live-text area: the user
 * has it switched on, and the current speech engine can produce live text
 * (the backend reports that, since it depends on the model and the GPU).
 * Custom overlays opt in through their HTML instead; see Overlay.svelte.
 */
export const liveTextShown = derived([config, status], ([$config, $status]) =>
  ($config.ui.show_live_text ?? true) && $status.live_text_capable !== false
);
