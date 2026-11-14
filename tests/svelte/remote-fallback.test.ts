import { describe, test, expect, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/svelte";
import { fallbackModel, fallbackOptions, statusSummary } from "../../src/lib/Settings/remote-fallback";
import EngineTab from "../../src/lib/Settings/EngineTab.svelte";

const status = vi.hoisted(() => ({ value: null as any }));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd: string, args: any) => {
    if (cmd === "get_remote_status") return status.value;
    // Only Parakeet's default model is on disk.
    if (cmd === "check_parakeet_downloaded") return args.modelSize === "tdt-0.6b-v3";
    if (cmd === "check_model_downloaded") return false;
    if (cmd === "check_moonshine_downloaded") return false;
    if (cmd === "moonshine_available" || cmd === "parakeet_available") return true;
    return null;
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

const engine = {
  backend: "remote-openai",
  whisper_cpp: { model_dir: "", model_size: "small", device: "auto", threads: 0 },
  moonshine: { model_size: "base", language: "en", device: "cpu" },
  parakeet: { model_size: "tdt-0.6b-v3", language: "auto", device: "cpu" },
  remote_openai: {
    endpoint: "http://192.168.1.50:8787/v1",
    api_key: "vx_key",
    model: "parakeet/tdt-0.6b-v3",
    language: "",
    timeout_secs: 30,
  },
  s1_mini: { enabled: false, styling: "semi-formal", gpu: true, app_styles: [] },
} as any;

function cfgWith(fallback?: string) {
  return {
    engine: {
      ...engine,
      remote_openai: { ...engine.remote_openai, ...(fallback ? { fallback } : {}) },
    },
  } as any;
}

describe("fallback choices", () => {
  test("each engine is offered with the model it would run", () => {
    const options = fallbackOptions(engine, { moonshine: true, parakeet: true });
    expect(options.map((o) => o.value)).toEqual(["none", "whisper-cpp", "moonshine", "parakeet"]);
    expect(options[1].label).toBe("Whisper.cpp (small)");
    expect(options[2].label).toBe("Moonshine (base)");
    expect(options[3].label).toBe("Parakeet TDT (tdt-0.6b-v3)");
  });

  test("an engine this build lacks is not offered", () => {
    const options = fallbackOptions(engine, { moonshine: false, parakeet: false });
    expect(options.map((o) => o.value)).toEqual(["none", "whisper-cpp"]);
  });

  test("the fallback's model is the one set in that engine's section", () => {
    expect(fallbackModel(engine, "parakeet")).toBe("tdt-0.6b-v3");
    expect(fallbackModel(engine, "whisper-cpp")).toBe("small");
    expect(fallbackModel(engine, "none")).toBeNull();
    expect(fallbackModel(engine, undefined)).toBeNull();
  });
});

describe("server status line", () => {
  test("says what happens to dictation while the server is away", () => {
    expect(statusSummary({ state: "online", reason: null, fallback: null })).toEqual({
      tone: "ok",
      text: "Connected to the server.",
    });
    expect(
      statusSummary({ state: "offline", reason: "Could not connect to the server", fallback: "Parakeet tdt-0.6b-v3" }),
    ).toEqual({
      tone: "warn",
      text: "Server unreachable: Could not connect to the server. Dictation is using the fallback engine (Parakeet tdt-0.6b-v3).",
    });
    expect(statusSummary({ state: "offline", reason: null, fallback: null })?.text).toBe(
      "Server unreachable: The server could not be reached. Dictation will fail until it is back.",
    );
    expect(statusSummary({ state: "unknown", reason: null, fallback: null })?.tone).toBe("muted");
    expect(statusSummary({ state: "not_in_use", reason: null, fallback: null })?.text).toContain("Save your settings");
    expect(statusSummary(null)).toBeNull();
  });
});

describe("EngineTab remote fallback", () => {
  test("a config from before the fallback existed shows None", async () => {
    status.value = { state: "online", reason: null, fallback: null };
    const cfg = cfgWith();
    render(EngineTab, { cfg });
    expect(await screen.findByText("Fallback engine")).not.toBeNull();
    expect(await screen.findByText("None — dictation fails with an error")).not.toBeNull();
    await waitFor(() => expect(screen.getByTestId("remote-status").textContent).toContain("Connected to the server."));
    expect(screen.queryByTestId("remote-fallback-missing")).toBeNull();
  });

  test("shows the offline status and which engine is covering", async () => {
    status.value = { state: "offline", reason: "Could not connect to the server", fallback: "Parakeet tdt-0.6b-v3" };
    render(EngineTab, { cfg: cfgWith("parakeet") });
    await waitFor(() =>
      expect(screen.getByTestId("remote-status").textContent).toContain(
        "Server unreachable: Could not connect to the server. Dictation is using the fallback engine (Parakeet tdt-0.6b-v3).",
      ),
    );
    // Parakeet's model is downloaded, so there is nothing to warn about.
    expect(screen.queryByTestId("remote-fallback-missing")).toBeNull();
  });

  test("warns when the chosen fallback's model is not downloaded", async () => {
    status.value = { state: "online", reason: null, fallback: "Whisper.cpp small" };
    render(EngineTab, { cfg: cfgWith("whisper-cpp") });
    const warning = await screen.findByTestId("remote-fallback-missing");
    expect(warning.textContent).toContain("The fallback model (small) is not downloaded");
    expect(warning.querySelector("button")?.textContent).toContain("Download");
  });
});
