import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import ServerSetupPanel from "../../src/lib/Settings/ServerSetupPanel.svelte";
import {
  connectionSummary,
  localBackendToRestore,
  normalizeServerAddress,
  serverUses,
  setSpeechOnServer,
} from "../../src/lib/Settings/server-setup";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function config(opts: { backend?: string; endpoint?: string; ttsEngine?: string; ttsEnabled?: boolean; noServer?: boolean } = {}) {
  return {
    engine: {
      backend: opts.backend ?? "parakeet",
      whisper_cpp: { model_size: "base" },
      moonshine: { model_size: "base" },
      parakeet: { model_size: "tdt-0.6b-v3" },
      remote_openai: opts.noServer
        ? undefined
        : { endpoint: opts.endpoint ?? "", api_key: null, model: "whisper-1", language: "", timeout_secs: 30, fallback: "none" },
    },
    tts: { enabled: opts.ttsEnabled ?? false, engine: opts.ttsEngine ?? "piper" },
  } as any;
}

let calls: { cmd: string; args: any }[] = [];
let stt: any;
let cleanup: any;
let speech: any;

beforeEach(() => {
  calls = [];
  stt = { success: true, message: "Connected to the server.", models: ["parakeet-tdt-0.6b-v3"] };
  cleanup = { available: true, message: "This server cleans up dictation (s1-mini)." };
  speech = { available: true, message: "This server offers 2 speech engines." };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    if (cmd === "test_remote_stt") return stt;
    if (cmd === "check_server_cleanup") return cleanup;
    if (cmd === "check_server_tts") return speech;
    return true;
  });
});

describe("server setup rules", () => {
  test("a bare machine name or IP becomes a full address, and a full one is left alone", () => {
    expect(normalizeServerAddress("192.168.1.162")).toBe("http://192.168.1.162:8787/v1");
    expect(normalizeServerAddress(" mybox ")).toBe("http://mybox:8787/v1");
    expect(normalizeServerAddress("192.168.1.162:9000")).toBe("http://192.168.1.162:9000/v1");
    expect(normalizeServerAddress("http://192.168.1.162:8787/v1/")).toBe("http://192.168.1.162:8787/v1");
    expect(normalizeServerAddress("https://speech.example.com")).toBe("https://speech.example.com:8787/v1");
    expect(normalizeServerAddress("https://speech.example.com/voxctrl")).toBe("https://speech.example.com/voxctrl/v1");
    expect(normalizeServerAddress("")).toBe("");
  });

  test("cleanup only counts as on the server while recognition is", () => {
    const cfg = config({ backend: "parakeet", endpoint: "http://box:8787/v1" });
    cfg.engine.remote_openai.cleanup_on_server = true;
    expect(serverUses(cfg)).toEqual({ recognition: false, cleanup: false, speech: false });
    cfg.engine.backend = "remote-openai";
    expect(serverUses(cfg).cleanup).toBe(true);
  });

  test("turning recognition off goes back to what was in use, then the fallback, then Whisper", () => {
    const cfg = config({ backend: "remote-openai" });
    expect(localBackendToRestore(cfg, "moonshine")).toBe("moonshine");
    expect(localBackendToRestore(cfg, null)).toBe("whisper-cpp");
    cfg.engine.remote_openai.fallback = "parakeet";
    expect(localBackendToRestore(cfg, null)).toBe("parakeet");
  });

  test("speech on the server switches text-to-speech on, and off again restores the engine", () => {
    const cfg = config({ ttsEngine: "kitten_tts_2", ttsEnabled: false });
    setSpeechOnServer(cfg, true, "kitten_tts_2");
    expect(cfg.tts).toMatchObject({ engine: "server", enabled: true });
    setSpeechOnServer(cfg, false, "kitten_tts_2");
    expect(cfg.tts.engine).toBe("kitten_tts_2");
    setSpeechOnServer(cfg, false, null);
    expect(cfg.tts.engine).toBe("espeak");
  });

  test("the summary names what the server offers, or says why it could not connect", () => {
    const base = { recognition: { ok: true, message: "", models: [] } };
    expect(
      connectionSummary({ ...base, cleanup: { available: true, message: "" }, speech: { available: true, message: "" } }).text,
    ).toBe("Connected. This server offers speech recognition, dictation cleanup and text-to-speech.");
    expect(
      connectionSummary({ ...base, cleanup: { available: false, message: "" }, speech: { available: false, message: "" } }).text,
    ).toBe("Connected. This server offers speech recognition.");
    const failed = connectionSummary({
      recognition: { ok: false, message: "Could not connect to the server.", models: [] },
      cleanup: { available: false, message: "" },
      speech: { available: false, message: "" },
    });
    expect(failed).toEqual({ ok: false, text: "Could not connect to the server." });
  });
});

describe("ServerSetupPanel", () => {
  test("with no server set, nothing can be switched on until an address is entered", async () => {
    render(ServerSetupPanel, { cfg: config({ noServer: true }) });
    expect((screen.getByTestId("server-connect") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("server-use-recognition") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId("server-use-speech") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId("server-address") as HTMLInputElement).value).toBe("");
  });

  test("connecting fills in the address, saves it with the key, and reports what the server offers", async () => {
    const cfg = config();
    render(ServerSetupPanel, { cfg });
    await fireEvent.input(screen.getByTestId("server-address"), { target: { value: "192.168.1.162" } });
    await fireEvent.input(screen.getByTestId("server-key"), { target: { value: " vx_secret " } });
    await fireEvent.click(screen.getByTestId("server-connect"));

    await waitFor(() =>
      expect(screen.getByTestId("server-summary").textContent).toContain(
        "This server offers speech recognition, dictation cleanup and text-to-speech.",
      ),
    );
    expect(cfg.engine.remote_openai.endpoint).toBe("http://192.168.1.162:8787/v1");
    expect(cfg.engine.remote_openai.api_key).toBe("vx_secret");
    // The speech model's name comes from the server, not from the user.
    expect(cfg.engine.remote_openai.model).toBe("parakeet-tdt-0.6b-v3");
    expect(calls.find((c) => c.cmd === "test_remote_stt")!.args).toMatchObject({
      endpoint: "http://192.168.1.162:8787/v1",
      apiKey: "vx_secret",
    });
    // Connecting changes what is known about the server, not what it is used for.
    expect(cfg.engine.backend).toBe("parakeet");
  });

  test("a failed connection says why", async () => {
    stt = { success: false, message: "The server rejected the API key (HTTP 401).", models: [] };
    render(ServerSetupPanel, { cfg: config({ endpoint: "http://box:8787/v1" }) });
    await fireEvent.click(screen.getByTestId("server-connect"));
    await waitFor(() =>
      expect(screen.getByTestId("server-summary").textContent).toContain("rejected the API key"),
    );
  });

  test("the switches turn the server on for recognition, cleanup and speech, and off again", async () => {
    const cfg = config({ backend: "parakeet", endpoint: "http://box:8787/v1", ttsEngine: "kitten_tts_2", ttsEnabled: true });
    render(ServerSetupPanel, { cfg });
    const recognition = screen.getByTestId("server-use-recognition") as HTMLInputElement;
    const cleanupBox = screen.getByTestId("server-use-cleanup") as HTMLInputElement;
    const speechBox = screen.getByTestId("server-use-speech") as HTMLInputElement;

    // Cleanup depends on recognition.
    expect(cleanupBox.disabled).toBe(true);
    await fireEvent.click(recognition);
    expect(cfg.engine.backend).toBe("remote-openai");
    await waitFor(() => expect(cleanupBox.disabled).toBe(false));
    expect(screen.getByText("If the server can't be reached")).toBeTruthy();

    await fireEvent.click(cleanupBox);
    expect(cfg.engine.remote_openai.cleanup_on_server).toBe(true);

    await fireEvent.click(speechBox);
    expect(cfg.tts.engine).toBe("server");

    // Off again: back to what was in use when the panel opened.
    await fireEvent.click(speechBox);
    expect(cfg.tts.engine).toBe("kitten_tts_2");
    await fireEvent.click(recognition);
    expect(cfg.engine.backend).toBe("parakeet");
    await waitFor(() => expect(cleanupBox.disabled).toBe(true));
    expect(cleanupBox.checked).toBe(false);
  });

  test("shows the current use of an already configured server", async () => {
    const cfg = config({ backend: "remote-openai", endpoint: "http://box:8787/v1", ttsEngine: "server", ttsEnabled: true });
    cfg.engine.remote_openai.cleanup_on_server = true;
    render(ServerSetupPanel, { cfg });
    expect((screen.getByTestId("server-address") as HTMLInputElement).value).toBe("http://box:8787/v1");
    expect((screen.getByTestId("server-use-recognition") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId("server-use-cleanup") as HTMLInputElement).checked).toBe(true);
    expect((screen.getByTestId("server-use-speech") as HTMLInputElement).checked).toBe(true);
  });

  test("what the server lacks is explained next to the switch", async () => {
    cleanup = { available: false, message: "Cleanup is turned off on this server." };
    speech = { available: false, message: "Text-to-speech is turned off on this server." };
    render(ServerSetupPanel, { cfg: config({ backend: "remote-openai", endpoint: "http://box:8787/v1" }) });
    await fireEvent.click(screen.getByTestId("server-connect"));
    await waitFor(() => expect(screen.getByText("Cleanup is turned off on this server.")).toBeTruthy());
    expect(screen.getByText("Text-to-speech is turned off on this server.")).toBeTruthy();
    expect(screen.getByTestId("server-summary").textContent).toContain("This server offers speech recognition.");
  });
});
