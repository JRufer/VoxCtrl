import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}));

import HistoryTab from "../../src/lib/Settings/HistoryTab.svelte";

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

function cfg(history: Record<string, unknown> = {}) {
  return {
    tts: { enabled: false },
    history: { enabled: true, persist: false, max_entries: 50, max_age_hours: 24, mcp_access: false, ...history },
  } as any;
}

function mockBackend(entries: Entry[] = ENTRIES, st = status()) {
  invokeMock.mockImplementation(async (cmd: string) => {
    switch (cmd) {
      case "get_history":
        return { entries, status: st };
      case "get_targets":
        return [];
      default:
        return null;
    }
  });
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("HistoryTab", () => {
  test("lists entries newest first with their target", async () => {
    mockBackend();
    render(HistoryTab, { props: { cfg: cfg() } });
    await vi.waitFor(() => expect(screen.getByText("second thought")).toBeTruthy());
    const texts = [...document.querySelectorAll(".entry .text")].map((e) => e.textContent);
    expect(texts).toEqual(["second thought", "first thought"]);
    expect(screen.getByText("→ Notes")).toBeTruthy();
  });

  test("shows an empty state", async () => {
    mockBackend([], status({ count: 0 }));
    render(HistoryTab, { props: { cfg: cfg() } });
    await vi.waitFor(() => expect(screen.getByText("Nothing dictated yet.")).toBeTruthy());
  });

  test("delete removes one entry through the backend", async () => {
    mockBackend();
    render(HistoryTab, { props: { cfg: cfg() } });
    await vi.waitFor(() => expect(screen.getByText("first thought")).toBeTruthy());
    const buttons = screen.getAllByRole("button", { name: "Delete this dictation" });
    await fireEvent.click(buttons[1]);
    expect(invokeMock).toHaveBeenCalledWith("delete_history_entry", { id: 1 });
    await vi.waitFor(() => expect(screen.queryByText("first thought")).toBeNull());
  });

  test("clear all asks for confirmation first", async () => {
    mockBackend();
    render(HistoryTab, { props: { cfg: cfg() } });
    await vi.waitFor(() => expect(screen.getByText("first thought")).toBeTruthy());
    await fireEvent.click(screen.getByRole("button", { name: "Clear all" }));
    expect(invokeMock).not.toHaveBeenCalledWith("clear_history");
    await fireEvent.click(screen.getByRole("button", { name: /Clear all 2/ }));
    expect(invokeMock).toHaveBeenCalledWith("clear_history");
  });

  test("type again goes through the backend, which hides Settings first", async () => {
    mockBackend();
    render(HistoryTab, { props: { cfg: cfg() } });
    await vi.waitFor(() => expect(screen.getByText("second thought")).toBeTruthy());
    await fireEvent.click(screen.getAllByRole("button", { name: "Type again" })[0]);
    expect(invokeMock).toHaveBeenCalledWith("paste_history_entry", { id: 2 });
  });

  test("a refused keyring is explained", async () => {
    mockBackend(ENTRIES, status({ persist_error: "no Secret Service on the session bus" }));
    render(HistoryTab, { props: { cfg: cfg({ persist: true }) } });
    await vi.waitFor(() => expect(screen.getByRole("alert").textContent).toContain("no Secret Service"));
    expect(screen.getByRole("alert").textContent).toContain("memory only");
  });

  test("with history off, nothing is listed", async () => {
    mockBackend();
    render(HistoryTab, { props: { cfg: cfg({ enabled: false }) } });
    await vi.waitFor(() => expect(invokeMock).toHaveBeenCalledWith("get_history", { query: "" }));
    expect(screen.queryByText("second thought")).toBeNull();
    expect(screen.queryByLabelText("Search history")).toBeNull();
  });
});
