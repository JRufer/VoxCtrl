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

// What a 0.9.11 server with two engines set up answers.
const twoEngines = {
  available: true,
  engine: "kitten_tts_2",
  voice: "Bruno",
  message: "This server offers 2 speech engines.",
  custom_voices: true,
  engines: [
    { id: "kitten_tts_2", name: "Kitten TTS 2", voices: ["Bruno", "Luna"], default_voice: "Bruno", custom_voices: true },
    { id: "piper", name: "Piper", voices: ["en-us-amy-low"], default_voice: "en-us-amy-low", custom_voices: false },
  ],
};
let ownVoices: string[] = [];
let syncResult: any = { local: [], uploaded: [], removed: [] };

let calls: { cmd: string; args: any }[] = [];
let status = { available: true, engine: "piper", voice: "en-us-amy-low", message: "This server speaks with piper, voice en-us-amy-low." };

beforeEach(() => {
  calls = [];
  status = { available: true, engine: "piper", voice: "en-us-amy-low", message: "This server speaks with piper, voice en-us-amy-low." };
  ownVoices = [];
  syncResult = { local: [], uploaded: [], removed: [] };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    if (cmd === "check_server_tts") return status;
    if (cmd === "list_own_voices") return ownVoices;
    if (cmd === "sync_server_voices") {
      if (syncResult instanceof Error) throw syncResult.message;
      return syncResult;
    }
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
      tlsFingerprint: null,
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

  test("a server from before engine choice shows its one engine and no pickers", async () => {
    render(TtsTab, { cfg: config("server") });
    await waitFor(() => expect(screen.getByTestId("server-tts-status").textContent).toContain("speaks with piper"));
    expect(screen.queryByTestId("server-voice-hint")).toBeNull();
  });
});

describe("TTS tab: choosing the server's engine and voice", () => {
  async function open(cfg: any) {
    status = twoEngines as any;
    const view = render(TtsTab, { cfg });
    await screen.findByTestId("server-voice-hint");
    // The tab's own engine picker is first; then the server's engine, then its voice.
    const triggers = () => Array.from(view.container.querySelectorAll(".custom-select-trigger")) as HTMLElement[];
    const panel = () => Array.from(screen.getByTestId("server-tts-panel").querySelectorAll(".custom-select-trigger")) as HTMLElement[];
    return { ...view, triggers, panel };
  }

  async function choose(trigger: HTMLElement, label: string) {
    await fireEvent.click(trigger);
    await fireEvent.click(await screen.findByText(label));
  }

  test("offers only the engines the server has set up, its default first", async () => {
    const { panel } = await open(config("server"));
    expect(panel()[0].textContent).toContain("Kitten TTS 2 (server default)");
    await fireEvent.click(panel()[0]);
    expect(await screen.findByText("Piper")).toBeTruthy();
    // Engines this server does not have are not offered.
    expect(screen.queryByText("VoxCPM2")).toBeNull();
  });

  test("the voice list is the chosen engine's, plus this computer's own clips where it can use them", async () => {
    ownVoices = ["Rick"];
    const cfg = config("server");
    const { panel } = await open(cfg);

    await fireEvent.click(panel()[1]);
    expect((await screen.findAllByText("Server default (Bruno)")).length).toBeGreaterThan(0);
    expect(screen.getByText("Luna")).toBeTruthy();
    expect(screen.getByText("Rick (your voice)")).toBeTruthy();
    await fireEvent.click(screen.getByText("Luna"));
    await waitFor(() => expect(cfg.tts.server_voice).toBe("Luna"));
    expect(calls.some((c) => c.cmd === "sync_server_voices")).toBe(false);
  });

  test("picking one of your own voices sends it to the server and saves the choice", async () => {
    ownVoices = ["Rick"];
    syncResult = { local: ["Rick"], uploaded: ["Rick"], removed: [] };
    const cfg = config("server");
    const { panel } = await open(cfg);

    await choose(panel()[1], "Rick (your voice)");
    await waitFor(() => expect(screen.getByTestId("voice-sync-note").textContent).toContain("sent Rick"));
    expect(cfg.tts.server_voice).toBe("custom:Rick");
    expect(calls.find((c) => c.cmd === "sync_server_voices")!.args).toEqual({
      endpoint: "http://box:8787/v1",
      apiKey: "vx_key",
      tlsFingerprint: null,
    });
  });

  test("a voice that could not be sent says so", async () => {
    ownVoices = ["Rick"];
    syncResult = new Error("The server did not accept the voice \"Rick\" (HTTP 400): The voice clip must be a WAV file.");
    const { panel } = await open(config("server"));
    await choose(panel()[1], "Rick (your voice)");
    await waitFor(() =>
      expect(screen.getByTestId("voice-sync-note").textContent).toContain("could not be sent to the server"),
    );
    expect(screen.getByTestId("voice-sync-note").textContent).toContain("must be a WAV file");
  });

  test("changing engine starts on that engine's default voice, and hides own voices where it can't use them", async () => {
    ownVoices = ["Rick"];
    const cfg = config("server");
    cfg.tts.server_voice = "Luna";
    const { panel } = await open(cfg);

    await choose(panel()[0], "Piper");
    await waitFor(() => expect(cfg.tts.server_engine).toBe("piper"));
    expect(cfg.tts.server_voice).toBe("");
    expect(screen.getByTestId("server-voice-hint").textContent).toContain("Piper cannot speak with an uploaded voice");

    await fireEvent.click(panel()[1]);
    expect(await screen.findByText("en-us-amy-low")).toBeTruthy();
    expect(screen.queryByText("Rick (your voice)")).toBeNull();
    expect(screen.queryByText("Send my voices now")).toBeNull();
  });

  test("a saved voice the server no longer has stays visible as unavailable", async () => {
    const cfg = config("server");
    cfg.tts.server_voice = "custom:Gone";
    const { panel } = await open(cfg);
    expect(panel()[1].textContent).toContain("Gone (not available)");
  });
});
