import { writable, derived } from "svelte/store";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";

export interface AppStatus {
  recording: boolean;
  speaking: boolean;
  word_count: number;
  active_target_id?: string;
  active_target_label?: string;
}

export const status = writable<AppStatus>({
  recording: false,
  speaking: false,
  word_count: 0,
  active_target_id: "default",
  active_target_label: "Focused Window",
});

export const recording = derived(status, ($s) => $s.recording);
export const speaking = derived(status, ($s) => $s.speaking);
export const wordCount = derived(status, ($s) => $s.word_count);
export const activeTargetLabel = derived(status, ($s) => $s.active_target_label ?? "Focused Window");

// Listen to periodic status ticks from the Rust backend
listen<AppStatus>("status-tick", (event) => {
  status.set(event.payload);
});

// Initial fetch
invoke<AppStatus>("get_status").then(status.set).catch(console.error);
