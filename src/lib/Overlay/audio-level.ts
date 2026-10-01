import { listen } from "@tauri-apps/api/event";

/**
 * Drive an overlay animation from the microphone level.
 *
 * Listens for the backend's `audio-level` events and calls `frame` once per
 * animation frame with a smoothed level in 0–1: a fast rise toward each new
 * reading and a steady decay between them, so the visual never jumps or
 * freezes between the (coalesced, ~60 Hz) level events.
 *
 * Returns the cleanup for `onMount`. The listener is released even when the
 * component is torn down before `listen()` has resolved — otherwise that
 * listener would be leaked for the life of the window.
 */
export function onLevelFrame(frame: (level: number) => void): () => void {
  let target = 0;
  let current = 0;
  let disposed = false;
  let unlisten: (() => void) | null = null;

  listen<number>("audio-level", (event) => {
    target = Math.min(1.0, event.payload * 100.0);
  }).then((fn) => {
    if (disposed) fn();
    else unlisten = fn;
  });

  let frameId = requestAnimationFrame(function tick() {
    current += (target - current) * 0.35;
    target *= 0.86;
    frame(current);
    frameId = requestAnimationFrame(tick);
  });

  return () => {
    disposed = true;
    unlisten?.();
    cancelAnimationFrame(frameId);
  };
}
