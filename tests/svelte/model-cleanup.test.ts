import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import ModelCleanupDialog from "../../src/lib/Settings/ModelCleanupDialog.svelte";
import {
  formatBytes,
  keptItems,
  removableItems,
  resultSummary,
  selectedBytes,
  type CleanupPlan,
} from "../../src/lib/Settings/model-cleanup";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const MB = 1024 * 1024;

// What the backend would list for a given fallback pick: the fallback
// engine's model is the one speech model that is kept.
function planFor(fallback: string): CleanupPlan {
  const items = [
    { id: "whisper:base", kind: "speech" as const, label: "Whisper.cpp base", bytes: 60 * MB, kept_because: null as string | null },
    { id: "parakeet:tdt-0.6b-v3", kind: "speech" as const, label: "Parakeet tdt-0.6b-v3", bytes: 640 * MB, kept_because: null as string | null },
    { id: "pocket-tts", kind: "voice" as const, label: "Pocket-TTS voice model", bytes: 250 * MB, kept_because: null as string | null },
  ];
  if (fallback === "parakeet") items[1].kept_because = "Fallback engine";
  if (fallback === "whisper-cpp") items[0].kept_because = "Fallback engine";
  return { items, removable_bytes: items.filter((i) => !i.kept_because).reduce((t, i) => t + i.bytes, 0) };
}

const choices = [
  { value: "none" as const, label: "None — dictation fails with an error" },
  { value: "whisper-cpp" as const, label: "Whisper.cpp (base)" },
  { value: "parakeet" as const, label: "Parakeet TDT (tdt-0.6b-v3)" },
];

function config(backend: string, fallback: string) {
  return {
    engine: {
      backend,
      whisper_cpp: { model_size: "base" },
      parakeet: { model_size: "tdt-0.6b-v3" },
      remote_openai: { fallback },
    },
  } as any;
}

let calls: { cmd: string; args: any }[] = [];

beforeEach(() => {
  calls = [];
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    // Copy: the dialog mutates its config after the call.
    calls.push({ cmd, args: JSON.parse(JSON.stringify(args ?? {})) });
    if (cmd === "plan_model_cleanup") return planFor(args.fallback);
    if (cmd === "run_model_cleanup") return { removed: args.ids, freed_bytes: 310 * MB, errors: [] };
    return undefined;
  });
});

describe("model cleanup helpers", () => {
  test("formats sizes at the precision a disk-space decision needs", () => {
    expect(formatBytes(0)).toBe("0 MB");
    expect(formatBytes(1000)).toBe("under 0.1 MB");
    expect(formatBytes(1.5 * MB)).toBe("1.5 MB");
    expect(formatBytes(312.4 * MB)).toBe("312 MB");
    expect(formatBytes(1434 * MB)).toBe("1.4 GB");
  });

  test("splits a plan into what can go and what is kept", () => {
    const plan = planFor("parakeet");
    expect(removableItems(plan).map((i) => i.id)).toEqual(["whisper:base", "pocket-tts"]);
    expect(keptItems(plan).map((i) => i.id)).toEqual(["parakeet:tdt-0.6b-v3"]);
    expect(removableItems(null)).toEqual([]);
  });

  test("totals only ticked models that can actually go", () => {
    const plan = planFor("parakeet");
    expect(selectedBytes(plan, new Set(["whisper:base", "parakeet:tdt-0.6b-v3"]))).toBe(60 * MB);
  });

  test("says what was removed and what could not be", () => {
    expect(resultSummary({ removed: ["a", "b"], freed_bytes: 300 * MB, errors: [] })).toEqual({
      ok: true,
      text: "Removed 2 models and freed 300 MB.",
    });
    const partial = resultSummary({ removed: ["a"], freed_bytes: 60 * MB, errors: ["Moonshine base: permission denied"] });
    expect(partial.ok).toBe(false);
    expect(partial.text).toBe(
      "Removed 1 model and freed 60 MB. 1 could not be removed: Moonshine base: permission denied",
    );
    expect(resultSummary({ removed: [], freed_bytes: 0, errors: ["x: busy"] }).text).toBe("1 could not be removed: x: busy");
  });
});

describe("ModelCleanupDialog", () => {
  test("with a local engine there is no fallback question, and everything removable starts ticked", async () => {
    render(ModelCleanupDialog, { cfg: config("parakeet", "none"), fallbackChoices: choices, onClose: () => {} });

    const run = await screen.findByTestId("cleanup-run");
    expect(screen.queryByText("Fallback engine")).toBeNull();
    expect(run.textContent).toContain("Remove 3 and free 950 MB");
    // Pending settings are written first: the list comes from what is saved.
    expect(calls.map((c) => c.cmd)).toEqual(["save_config", "plan_model_cleanup"]);
  });

  test("with a server, the saved fallback's model is listed as kept", async () => {
    render(ModelCleanupDialog, { cfg: config("remote-openai", "parakeet"), fallbackChoices: choices, onClose: () => {} });

    const kept = await screen.findByTestId("cleanup-kept");
    expect(kept.textContent).toContain("Parakeet tdt-0.6b-v3");
    expect(kept.textContent).toContain("Fallback engine");
    expect(screen.getByTestId("cleanup-removable").textContent).not.toContain("Parakeet");
    expect(screen.queryByTestId("cleanup-no-fallback")).toBeNull();
    expect(calls.find((c) => c.cmd === "plan_model_cleanup")!.args).toEqual({ fallback: "parakeet" });
  });

  test("with a server and no fallback, it warns that dictation will fail offline", async () => {
    render(ModelCleanupDialog, { cfg: config("remote-openai", "none"), fallbackChoices: choices, onClose: () => {} });
    expect((await screen.findByTestId("cleanup-no-fallback")).textContent).toContain("dictation fails with an error");
  });

  test("removes only the ticked models, by id", async () => {
    render(ModelCleanupDialog, { cfg: config("remote-openai", "parakeet"), fallbackChoices: choices, onClose: () => {} });
    await screen.findByTestId("cleanup-run");

    // Keep the Pocket-TTS model after all.
    const boxes = screen.getAllByRole("checkbox") as HTMLInputElement[];
    expect(boxes).toHaveLength(2);
    await fireEvent.click(boxes[1]);
    expect(screen.getByTestId("cleanup-run").textContent).toContain("Remove 1 and free 60 MB");

    await fireEvent.click(screen.getByTestId("cleanup-run"));
    await waitFor(() => expect(screen.getByTestId("cleanup-result")).toBeTruthy());

    const run = calls.find((c) => c.cmd === "run_model_cleanup")!;
    expect(run.args).toEqual({ ids: ["whisper:base"] });
    expect(screen.getByTestId("cleanup-result").textContent).toContain("Removed 1 model and freed 310 MB.");
    // The fallback was not changed, so nothing but the opening save happened.
    expect(calls.filter((c) => c.cmd === "save_config")).toHaveLength(1);
  });

  test("a fallback picked in the dialog is saved before anything is removed", async () => {
    const cfg = config("remote-openai", "none");
    const { container } = render(ModelCleanupDialog, { cfg, fallbackChoices: choices, onClose: () => {} });
    await screen.findByTestId("cleanup-run");

    // Pick Parakeet as the fallback.
    await fireEvent.click(container.querySelector(".custom-select-trigger") as HTMLElement);
    await fireEvent.click(await screen.findByText("Parakeet TDT (tdt-0.6b-v3)"));
    await waitFor(() => expect(screen.getByTestId("cleanup-kept").textContent).toContain("Parakeet"));

    await fireEvent.click(screen.getByTestId("cleanup-run"));
    await waitFor(() => expect(screen.getByTestId("cleanup-result")).toBeTruthy());

    const order = calls.map((c) => c.cmd);
    const saved = order.lastIndexOf("save_config");
    const removed = order.indexOf("run_model_cleanup");
    expect(saved).toBeGreaterThan(0);
    expect(saved).toBeLessThan(removed);
    expect(calls[saved].args.newConfig.engine.remote_openai.fallback).toBe("parakeet");
    expect(calls[removed].args.ids).toEqual(["whisper:base", "pocket-tts"]);
  });

  test("a failure to list models is shown, with nothing offered for removal", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "plan_model_cleanup") throw "could not read the models folder";
      return undefined;
    });
    render(ModelCleanupDialog, { cfg: config("parakeet", "none"), fallbackChoices: choices, onClose: () => {} });
    expect((await screen.findByTestId("cleanup-error")).textContent).toContain("could not read the models folder");
    expect(screen.queryByTestId("cleanup-run")).toBeNull();
  });
});
