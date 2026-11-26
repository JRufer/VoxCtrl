import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import TtsTab from "../../src/lib/Settings/TtsTab.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

function config(engine: string, audiocppServer = "") {
  return {
    engine: { backend: "whisper-cpp" },
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
      idle_unload_secs: 900,
      pocket_tts: { voice: "alba", voice_dir: "", gpu: false, prewarm: false },
      inflect_micro: { model_dir: "", seed: 0, noise_scale: 0.667, noise_scale_w: 0.8, prewarm: false },
      breeze_tts_2: { model_dir: "", voice_dir: "", voice_mode: "clone", cloned_voice: "", prompt: "", gpu: false, prewarm: false },
      vox_cpm_2: { model_dir: "", voice_dir: "", voice_mode: "clone", cloned_voice: "", prompt: "", gpu: false, prewarm: false, ultimate_cloning: false },
      kitten_tts_2: { model_dir: "", voice_dir: "", voice_mode: "preset", voice: "Bruno", cloned_voice: "", preset: "stable", gpu: false, prewarm: false },
      reverb: { voice_mode: "preset", voice: "en_female_narrator", cloned_voice: "", voice_dir: "", language: "", model_dir: "", prewarm: false, gpu: false },
      audiocpp_server: audiocppServer,
    },
  } as any;
}

let calls: { cmd: string; args: any }[] = [];
let runtimeOk = false;
let modelReady = false;

beforeEach(() => {
  calls = [];
  runtimeOk = false;
  modelReady = false;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    if (cmd === "reverb_runtime_supported") return runtimeOk;
    if (cmd === "check_reverb_ready") return runtimeOk && modelReady;
    if (cmd === "list_reverb_voices") return ["en_female_narrator", "de_male_standard"];
    if (cmd.startsWith("list_") || cmd.includes("voices") || cmd.includes("catalogue")) return [];
    if (cmd === "hf_token_env") return null;
    if (cmd === "accelerator_support") return {};
    return false;
  });
});

describe("TTS tab: Reverb-TTS", () => {
  test("is offered as an engine", async () => {
    const { container } = render(TtsTab, { cfg: config("espeak") });
    await fireEvent.click(container.querySelector(".custom-select-trigger") as HTMLElement);
    expect(await screen.findByText("Reverb-TTS (neural, 7 languages, voice cloning, fast)")).toBeTruthy();
  });

  test("says it needs an audio.cpp server with Reverb when the runtime lacks it", async () => {
    render(TtsTab, { cfg: config("reverb") });
    expect(await screen.findByText("⚠️ Needs an audio.cpp server with Reverb-TTS")).toBeTruthy();
    expect(await screen.findByText("❌ Not ready: the audio.cpp server has no Reverb-TTS")).toBeTruthy();
  });

  test("checks readiness against the audio.cpp server path as typed", async () => {
    runtimeOk = true;
    modelReady = true;
    render(TtsTab, { cfg: config("reverb", "/opt/acpp/audiocpp_server") });
    expect(await screen.findByText("✔ Model downloaded and ready")).toBeTruthy();
    const check = calls.find(c => c.cmd === "check_reverb_ready");
    expect(check?.args).toEqual({ modelDir: "", audiocppServer: "/opt/acpp/audiocpp_server" });
    expect(screen.queryByText("⚠️ Needs an audio.cpp server with Reverb-TTS")).toBeNull();
  });

  test("labels preset voices by language", async () => {
    runtimeOk = true;
    render(TtsTab, { cfg: config("reverb") });
    await waitFor(() => expect(calls.some(c => c.cmd === "list_reverb_voices")).toBe(true));
    expect(await screen.findByText("English · female narrator")).toBeTruthy();
  });
});
