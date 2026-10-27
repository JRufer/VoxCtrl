import { describe, test, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/svelte";
import { tick } from "svelte";

// Event listeners the components register, so tests can emit backend events.
// Hoisted: the module mocks below run before the rest of this file.
const { handlers } = vi.hoisted(() => ({
  handlers: new Map<string, Set<(e: { payload: unknown }) => void>>(),
}));
function emit(name: string, payload: unknown) {
  handlers.get(name)?.forEach((h) => h({ payload }));
}

// A custom overlay using every live-text hook, served by get_custom_overlay.
const CUSTOM_HTML = `
  <div class="whole" data-voxctrl-live-text></div>
  <div class="split"><span class="c" data-voxctrl-live-committed></span> <span class="t" data-voxctrl-live-tentative></span></div>
`;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args?: { name?: string }) => {
    if (cmd === "get_config") throw new Error("no backend in tests");
    if (cmd === "get_custom_overlay") {
      return args?.name === "Hooks" ? { name: "Hooks", html: CUSTOM_HTML, css: "" } : null;
    }
    const { status } = await import("../../src/stores/status");
    const { get } = await import("svelte/store");
    return get(status);
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, h: (e: { payload: unknown }) => void) => {
    if (!handlers.has(name)) handlers.set(name, new Set());
    handlers.get(name)!.add(h);
    return () => handlers.get(name)?.delete(h);
  }),
}));

vi.stubGlobal(
  "requestAnimationFrame",
  (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16) as unknown as number
);
vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));

import { status, type AppStatus } from "../../src/stores/status";
import { config } from "../../src/stores/config";
import { liveTranscript, EMPTY_LIVE_TRANSCRIPT, liveFullText } from "../../src/stores/liveTranscript";
import Overlay from "../../src/lib/Overlay/Overlay.svelte";
import VoiceCard from "../../src/lib/Overlay/VoiceCard.svelte";
import Terminal from "../../src/lib/Overlay/Terminal.svelte";

function setStatus(partial: Partial<AppStatus> = {}) {
  status.set({
    recording: true,
    processing: false,
    speaking: false,
    mcp_recording: false,
    audio_ready: true,
    word_count: 0,
    active_target_id: "default",
    active_target_label: "Focused Window",
    live_text_capable: true,
    ...partial,
  });
}

function setLiveSetting(on: boolean, style = "voice_card") {
  config.update((c) => ({ ...c, ui: { ...c.ui, show_overlay: true, show_live_text: on, overlay_style: style } }));
}

beforeEach(() => {
  setStatus();
  setLiveSetting(true);
  liveTranscript.set({ ...EMPTY_LIVE_TRANSCRIPT });
});

describe("live transcript store", () => {
  test("joins committed and tentative words", () => {
    expect(liveFullText({ session_id: 1, committed: "Let me draft", tentative: "a note", final: false })).toBe(
      "Let me draft a note"
    );
    expect(liveFullText({ session_id: 1, committed: "", tentative: "a note", final: false })).toBe("a note");
  });
});

describe("built-in overlays", () => {
  test("show committed words and dim tentative ones", async () => {
    const { container } = render(VoiceCard, { recording: true, active: true });
    liveTranscript.set({ session_id: 1, committed: "Let me draft a", tentative: "quick note", final: false });
    await tick();
    expect(container.querySelector(".lt-committed")?.textContent).toBe("Let me draft a");
    expect(container.querySelector(".lt-tentative")?.textContent).toBe("quick note");
  });

  test("show a placeholder until words arrive", () => {
    const { container } = render(VoiceCard, { recording: true, active: true });
    expect(container.querySelector(".lt-placeholder")?.textContent).toBe("Say something…");
  });

  test("have no live-text area with the setting off", () => {
    setLiveSetting(false);
    const { container } = render(VoiceCard, { recording: true, active: true });
    expect(container.querySelector(".lt")).toBeNull();
    expect(container.querySelector(".card")?.classList.contains("live")).toBe(false);
  });

  test("have no live-text area when the engine cannot produce it", () => {
    setStatus({ live_text_capable: false });
    const { container } = render(Terminal, { recording: true, active: true });
    expect(container.querySelector(".lt")).toBeNull();
  });
});

describe("Overlay.svelte live-transcript events", () => {
  test("fill a custom overlay's whole, committed and tentative hooks", async () => {
    setLiveSetting(true, "Hooks");
    const { container } = render(Overlay);
    await vi.waitFor(() => expect(container.querySelector("[data-voxctrl-live-committed]")).not.toBeNull());

    emit("live-transcript", {
      session_id: 3,
      committed: "push it to",
      tentative: "Thursday",
      text: "push it to Thursday",
      final: false,
    });
    await tick();

    expect(container.querySelector(".whole")?.textContent).toBe("push it to Thursday");
    expect(container.querySelector(".c")?.textContent).toBe("push it to");
    expect(container.querySelector(".t")?.textContent).toBe("Thursday");
    const root = document.documentElement.style;
    expect(root.getPropertyValue("--voxctrl-has-live-text")).toBe("1");
    expect(root.getPropertyValue("--voxctrl-live-split")).toBe("1");
    expect(root.getPropertyValue("--voxctrl-live-enabled")).toBe("1");
    expect(root.getPropertyValue("--voxctrl-live-final")).toBe("0");
  });

  test("mark the final transcript and ignore a stale session", async () => {
    render(Overlay);
    emit("live-transcript", { session_id: 5, committed: "Final words.", tentative: "", final: true });
    await tick();
    expect(document.documentElement.style.getPropertyValue("--voxctrl-live-final")).toBe("1");

    // A late pass from the previous recording must not overwrite it.
    emit("live-transcript", { session_id: 4, committed: "", tentative: "stale", final: false });
    await tick();
    expect(document.documentElement.style.getPropertyValue("--voxctrl-live-final")).toBe("1");
  });

  test("accept the older text-only payload as tentative words", async () => {
    render(Overlay);
    emit("live-transcript", { session_id: 9, text: "older backend" });
    await tick();
    const { get } = await import("svelte/store");
    const t = get(liveTranscript);
    expect(t.committed).toBe("");
    expect(t.tentative).toBe("older backend");
  });

  test("tell custom overlays when live text is switched off", async () => {
    setLiveSetting(false, "Hooks");
    render(Overlay);
    await tick();
    expect(document.documentElement.style.getPropertyValue("--voxctrl-live-enabled")).toBe("0");
  });
});
