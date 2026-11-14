import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import EngineTab from "../../src/lib/Settings/EngineTab.svelte";
import FeaturesTab from "../../src/lib/Settings/FeaturesTab.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

function config(opts: { backend?: string; cleanupOnServer?: boolean; s1Enabled?: boolean } = {}) {
  return {
    engine: {
      backend: opts.backend ?? "remote-openai",
      whisper_cpp: { model_dir: "", model_size: "base", device: "auto", threads: 0 },
      moonshine: { model_size: "base", language: "en", device: "auto" },
      parakeet: { model_size: "tdt-0.6b-v3", language: "auto", device: "cpu" },
      remote_openai: {
        endpoint: "http://box:8787/v1",
        api_key: "vx_key",
        model: "whisper-1",
        language: "",
        timeout_secs: 30,
        fallback: "none",
        cleanup_on_server: opts.cleanupOnServer,
      },
      s1_mini: { enabled: opts.s1Enabled ?? false, styling: "semi-formal", gpu: true, app_styles: [] },
    },
    features: { custom_vocabulary: [], snippets: {} },
  } as any;
}

let calls: { cmd: string; args: any }[] = [];
let cleanupStatus = { available: true, model: "s1-mini", message: "This server cleans up dictation (s1-mini)." };

beforeEach(() => {
  calls = [];
  cleanupStatus = { available: true, model: "s1-mini", message: "This server cleans up dictation (s1-mini)." };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    switch (cmd) {
      case "check_server_cleanup":
        return cleanupStatus;
      case "get_remote_status":
        return { state: "online", reason: null, fallback: null };
      case "check_s1_mini_downloaded":
        return false;
      case "accelerator_support":
        return { s1_mini_gpu: null };
      case "paste_support":
        return { supported: true, reason: null };
      default:
        return true;
    }
  });
});

describe("Engine tab: cleanup on the server", () => {
  test("is off for a config from before the option existed, and asks the server nothing", async () => {
    render(EngineTab, { cfg: config({ cleanupOnServer: undefined }) });
    const box = (await screen.findByTestId("cleanup-on-server")) as HTMLInputElement;
    expect(box.checked).toBe(false);
    expect(screen.queryByTestId("server-cleanup-status")).toBeNull();
    expect(calls.some((c) => c.cmd === "check_server_cleanup")).toBe(false);
  });

  test("switching it on checks the server at the typed address and shows the answer", async () => {
    const cfg = config();
    render(EngineTab, { cfg });
    await fireEvent.click(await screen.findByTestId("cleanup-on-server"));

    await waitFor(() =>
      expect(screen.getByTestId("server-cleanup-status").textContent).toContain(
        "This server cleans up dictation (s1-mini).",
      ),
    );
    expect(cfg.engine.remote_openai.cleanup_on_server).toBe(true);
    expect(calls.find((c) => c.cmd === "check_server_cleanup")!.args).toEqual({
      endpoint: "http://box:8787/v1",
      apiKey: "vx_key",
    });
  });

  test("a server that can't do it is reported with what to change, and can be asked again", async () => {
    cleanupStatus = { available: false, model: null, message: "Cleanup is turned off on this server." };
    render(EngineTab, { cfg: config({ cleanupOnServer: true }) });

    // On already, so the tab asks as it opens.
    await waitFor(() =>
      expect(screen.getByTestId("server-cleanup-status").textContent).toContain("Cleanup is turned off on this server."),
    );

    cleanupStatus = { available: true, model: "s1-mini", message: "This server cleans up dictation (s1-mini)." };
    await fireEvent.click(screen.getByText("Check again"));
    await waitFor(() =>
      expect(screen.getByTestId("server-cleanup-status").textContent).toContain("This server cleans up dictation"),
    );
    expect(screen.queryByText("Check again")).toBeNull();
  });

  test("the option is not shown with a local engine", async () => {
    render(EngineTab, { cfg: config({ backend: "parakeet", cleanupOnServer: true }) });
    await screen.findByText("Downloaded Models");
    expect(screen.queryByTestId("cleanup-on-server")).toBeNull();
    expect(calls.some((c) => c.cmd === "check_server_cleanup")).toBe(false);
  });
});

describe("Features tab: cleanup on the server", () => {
  test("says cleanup runs on the server and offers no local model, but keeps the style", async () => {
    render(FeaturesTab, { cfg: config({ cleanupOnServer: true, s1Enabled: true }) });

    expect((await screen.findByTestId("cleanup-on-server-note")).textContent).toContain("runs on your speech server");
    expect(screen.queryByText(/Model files missing/)).toBeNull();
    expect(screen.queryByText(/480 MB download/)).toBeNull();
    expect(screen.getByText("Cleanup styling")).toBeTruthy();
  });

  test("switching cleanup on does not download the model when the server does the work", async () => {
    const cfg = config({ cleanupOnServer: true, s1Enabled: false });
    render(FeaturesTab, { cfg });
    await screen.findByTestId("cleanup-on-server-note");

    await fireEvent.click(screen.getByLabelText(/Enable S1-mini dictation cleanup/));
    await waitFor(() => expect(cfg.engine.s1_mini.enabled).toBe(true));
    expect(calls.some((c) => c.cmd === "download_s1_mini_model")).toBe(false);
  });

  test("with cleanup on this computer, the model status and download are shown as before", async () => {
    render(FeaturesTab, { cfg: config({ cleanupOnServer: false, s1Enabled: true }) });
    expect(await screen.findByText(/Model files missing/)).toBeTruthy();
    expect(screen.queryByTestId("cleanup-on-server-note")).toBeNull();
  });

  test("a local engine ignores a leftover server setting", async () => {
    render(FeaturesTab, { cfg: config({ backend: "parakeet", cleanupOnServer: true, s1Enabled: true }) });
    expect(await screen.findByText(/Model files missing/)).toBeTruthy();
    expect(screen.queryByTestId("cleanup-on-server-note")).toBeNull();
  });
});
