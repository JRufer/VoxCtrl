import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import TtsTab from "../../src/lib/Settings/TtsTab.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

function config(engine: string) {
  return {
    engine: {
      backend: "remote-openai",
      remote_openai: { endpoint: "http://box:8787/v1", api_key: "vx_key", model: "whisper-1" },
    },
    tts: {
      enabled: true,
      engine,
      voice: "en-us-lessac-medium",
      voice_dir: "",
      stop_key: ["KEY_ESC"],
      response_overlay: true,
      speed: 1.0,
      gpu: false,
      hf_token: null,
      snippets: {},
      memory_mode: "always_loaded",
      idle_unload_minutes: 15,
      pocket_tts: { voice: "alba", voice_dir: "", gpu: false, prewarm: false },
      inflect_micro: { model_dir: "", seed: 0, noise_scale: 0.667, noise_scale_w: 0.8, prewarm: false },
      breeze_tts_2: { model_dir: "", voice_dir: "", voice_mode: "clone", cloned_voice: "", prompt: "", gpu: false, prewarm: false },
      vox_cpm_2: { model_dir: "", voice_dir: "", voice_mode: "clone", cloned_voice: "", prompt: "", gpu: false, prewarm: false, ultimate_cloning: false },
      kitten_tts_2: { model_dir: "", voice_dir: "", voice_mode: "preset", voice: "Bella", cloned_voice: "", preset: "balanced", gpu: false, prewarm: false },
    },
  } as any;
}

let calls: { cmd: string; args: any }[] = [];
let status = { available: true, engine: "piper", voice: "en-us-amy-low", message: "This server speaks with piper, voice en-us-amy-low." };

beforeEach(() => {
  calls = [];
  status = { available: true, engine: "piper", voice: "en-us-amy-low", message: "This server speaks with piper, voice en-us-amy-low." };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    if (cmd === "check_server_tts") return status;
    if (cmd.startsWith("list_") || cmd.includes("voices") || cmd.includes("catalogue")) return [];
    if (cmd === "hf_token_env") return null;
    if (cmd === "accelerator_support") return {};
    return false;
  });
});

describe("TTS tab: VoxCtrl server engine", () => {
  test("is offered as an engine", async () => {
    const { container } = render(TtsTab, { cfg: config("espeak") });
    await fireEvent.click(container.querySelector(".custom-select-trigger") as HTMLElement);
    expect(await screen.findByText("VoxCtrl server (speaks on your speech server)")).toBeTruthy();
    expect(screen.queryByTestId("server-tts-panel")).toBeNull();
  });

  test("asks the server set under Engine what it speaks with, and shows the answer", async () => {
    render(TtsTab, { cfg: config("server") });
    await waitFor(() =>
      expect(screen.getByTestId("server-tts-status").textContent).toContain(
        "This server speaks with piper, voice en-us-amy-low.",
      ),
    );
    expect(calls.find((c) => c.cmd === "check_server_tts")!.args).toEqual({
      endpoint: "http://box:8787/v1",
      apiKey: "vx_key",
    });
    // No local voice or model controls for this engine.
    expect(screen.queryByText("Piper Voice")).toBeNull();
  });

  test("a server that does not speak is reported with what to change, and can be asked again", async () => {
    status = { available: false, engine: null, voice: null, message: "Text-to-speech is turned off on this server." };
    render(TtsTab, { cfg: config("server") });
    await waitFor(() =>
      expect(screen.getByTestId("server-tts-status").textContent).toContain("turned off on this server"),
    );

    status = { available: true, engine: "espeak", voice: null, message: "This server speaks with espeak." };
    await fireEvent.click(screen.getByText("Check the server"));
    await waitFor(() =>
      expect(screen.getByTestId("server-tts-status").textContent).toContain("This server speaks with espeak."),
    );
  });

  test("the test button is usable without anything downloaded here", async () => {
    render(TtsTab, { cfg: config("server") });
    await screen.findByTestId("server-tts-panel");
    const button = screen.getByText("Test TTS") as HTMLButtonElement;
    await waitFor(() => expect(button.disabled).toBe(false));
  });
});
