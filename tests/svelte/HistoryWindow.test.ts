import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";

const { invokeMock, handlers } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  handlers: new Map<string, Set<(e: { payload: unknown }) => void>>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, h: (e: { payload: unknown }) => void) => {
    if (!handlers.has(name)) handlers.set(name, new Set());
    handlers.get(name)!.add(h);
    return () => handlers.get(name)?.delete(h);
  }),
  emit: vi.fn(),
}));

function emit(name: string, payload: unknown = null) {
  handlers.get(name)?.forEach((h) => h({ payload }));
}

import HistoryWindow from "../../src/lib/History/HistoryWindow.svelte";
import { config } from "../../src/stores/config";

type Entry = { id: number; at: number; target_id: string; target_label: string; text: string };

function status(over: Record<string, unknown> = {}) {
  return {
    enabled: true,
    persisting: false,
    persist_error: null,
    key_store: "the system keyring",
    count: 2,
    ...over,
  };
}

const ENTRIES: Entry[] = [
  { id: 2, at: Math.floor(Date.now() / 1000), target_id: "default", target_label: "Focused Window", text: "second thought" },
  { id: 1, at: Math.floor(Date.now() / 1000) - 60, target_id: "notes", target_label: "Notes", text: "first thought" },
];

function setConfig(history: Record<string, unknown> = {}, ttsEnabled = true) {
  // The store's own load ran against the mock at import and may have come
  // back empty; the window only reads these two sections.
  config.update((c) => ({
    ...(c ?? {}),
    tts: { ...(c?.tts ?? {}), enabled: ttsEnabled },
    history: { enabled: true, persist: false, max_entries: 50, max_age_hours: 24, mcp_access: false, ...history },
  }) as any);
}

function mockBackend(entries: Entry[] = ENTRIES, st = status(), speak?: () => Promise<unknown>) {
  invokeMock.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "get_history":
        return { entries, status: st };
      case "get_targets":
        return [];
      case "speak_history_entry":
        return speak ? speak() : null;
      default:
        return null;
    }
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  handlers.clear();
  setConfig();
});

describe("HistoryWindow", () => {
  test("lists entries newest first with their target", async () => {
    mockBackend();
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText("second thought")).toBeTruthy());
    const texts = [...document.querySelectorAll(".entry .text")].map((e) => e.textContent);
    expect(texts).toEqual(["second thought", "first thought"]);
    expect(screen.getByText("→ Notes")).toBeTruthy();
  });

  test("shows an empty state", async () => {
    mockBackend([], status({ count: 0 }));
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText("Nothing dictated yet.")).toBeTruthy());
  });

  test("delete removes one entry through the backend", async () => {
    mockBackend();
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText("first thought")).toBeTruthy());
    await fireEvent.click(screen.getAllByRole("button", { name: "Delete this dictation" })[1]);
    expect(invokeMock).toHaveBeenCalledWith("delete_history_entry", { id: 1 });
    await vi.waitFor(() => expect(screen.queryByText("first thought")).toBeNull());
  });

  test("clear all asks for confirmation first", async () => {
    mockBackend();
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText("first thought")).toBeTruthy());
    await fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(invokeMock).not.toHaveBeenCalledWith("clear_history");
    await fireEvent.click(screen.getByRole("button", { name: /Clear all 2/ }));
    expect(invokeMock).toHaveBeenCalledWith("clear_history");
  });

  test("type again goes through the backend, which hides the window first", async () => {
    mockBackend();
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText("second thought")).toBeTruthy());
    await fireEvent.click(screen.getAllByRole("button", { name: "Type again" })[0]);
    expect(invokeMock).toHaveBeenCalledWith("paste_history_entry", { id: 2 });
  });

  test("privacy settings are tucked away and explain a refused keyring", async () => {
    mockBackend(ENTRIES, status({ persist_error: "no Secret Service on the session bus" }));
    setConfig({ persist: true });
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText("second thought")).toBeTruthy());
    expect(screen.queryByRole("alert")).toBeNull();
    await fireEvent.click(screen.getByRole("button", { name: "Privacy settings" }));
    expect(screen.getByRole("alert").textContent).toContain("no Secret Service");
    expect(screen.getByText("Let MCP clients read recent dictations")).toBeTruthy();
  });

  test("with history off, nothing is listed", async () => {
    mockBackend();
    setConfig({ enabled: false });
    render(HistoryWindow);
    await vi.waitFor(() => expect(screen.getByText(/History is off, so nothing/)).toBeTruthy());
    expect(screen.queryByText("second thought")).toBeNull();
    expect(screen.queryByLabelText("Search history")).toBeNull();
  });

  describe("Read aloud", () => {
    async function ready() {
      render(HistoryWindow);
      await vi.waitFor(() => expect(screen.getByText("second thought")).toBeTruthy());
      // Listeners register after mount.
      await vi.waitFor(() => expect(handlers.get("tts-playback-end")?.size).toBe(1));
    }

    test("shows it's preparing, then speaking, then goes back to a button", async () => {
      mockBackend();
      await ready();
      await fireEvent.click(screen.getAllByRole("button", { name: "Read aloud" })[0]);
      expect(invokeMock).toHaveBeenCalledWith("speak_history_entry", { id: 2 });

      const busy = await screen.findByRole("button", { name: /Preparing voice/ });
      expect((busy as HTMLButtonElement).disabled).toBe(true);
      // The other entry can't start a second read meanwhile.
      const other = screen.getByRole("button", { name: "Read aloud" }) as HTMLButtonElement;
      expect(other.disabled).toBe(true);

      emit("tts-playback-start");
      expect(await screen.findByRole("button", { name: /Speaking · Stop/ })).toBeTruthy();

      emit("tts-playback-end");
      await vi.waitFor(() => expect(screen.getAllByRole("button", { name: "Read aloud" })).toHaveLength(2));
      expect(screen.getAllByRole("button", { name: "Read aloud" }).every((b) => !(b as HTMLButtonElement).disabled)).toBe(true);
    });

    test("clicking again does not queue a second read", async () => {
      mockBackend();
      await ready();
      await fireEvent.click(screen.getAllByRole("button", { name: "Read aloud" })[0]);
      await fireEvent.click(await screen.findByRole("button", { name: /Preparing voice/ }));
      expect(invokeMock.mock.calls.filter(([c]) => c === "speak_history_entry")).toHaveLength(1);
    });

    test("stop ends playback and restores the button", async () => {
      mockBackend();
      await ready();
      await fireEvent.click(screen.getAllByRole("button", { name: "Read aloud" })[0]);
      emit("tts-playback-start");
      await fireEvent.click(await screen.findByRole("button", { name: /Speaking · Stop/ }));
      expect(invokeMock).toHaveBeenCalledWith("stop_tts");
      await vi.waitFor(() => expect(screen.getAllByRole("button", { name: "Read aloud" })).toHaveLength(2));
    });

    test("a failure is shown and the button comes back", async () => {
      mockBackend(ENTRIES, status(), async () => {
        throw "Text-to-speech is off. Turn it on in Settings → TTS.";
      });
      await ready();
      await fireEvent.click(screen.getAllByRole("button", { name: "Read aloud" })[0]);
      expect(await screen.findByText(/Text-to-speech is off/)).toBeTruthy();
      expect(screen.getAllByRole("button", { name: "Read aloud" })).toHaveLength(2);
    });

    test("an engine error while preparing is shown and the button comes back", async () => {
      mockBackend();
      await ready();
      await fireEvent.click(screen.getAllByRole("button", { name: "Read aloud" })[0]);
      await screen.findByRole("button", { name: /Preparing voice/ });
      emit("tts-error", "voice not downloaded");
      expect(await screen.findByText(/voice not downloaded/)).toBeTruthy();
      expect(screen.getAllByRole("button", { name: "Read aloud" })).toHaveLength(2);
    });
  });
});
