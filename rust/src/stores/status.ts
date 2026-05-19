import { writable, derived } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  recording: boolean;
  speaking: boolean;
  word_count: number;
}

export const status = writable<AppStatus>({
  recording: false,
  speaking: false,
  word_count: 0,
});

export const recording = derived(status, ($s) => $s.recording);
export const speaking = derived(status, ($s) => $s.speaking);
export const wordCount = derived(status, ($s) => $s.word_count);

// Listen to periodic status ticks from the Rust backend
listen<AppStatus>("status-tick", (event) => {
  status.set(event.payload);
});

// Initial fetch
invoke<AppStatus>("get_status").then(status.set).catch(console.error);
