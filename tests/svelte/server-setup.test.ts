import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import EngineTab from "../../src/lib/Settings/EngineTab.svelte";
import { readFileSync } from "node:fs";
import {
  connectionSummary,
  normalizeServerAddress,
  setSpeechOnServer,
  shortFingerprint,
  speechOnServer,
} from "../../src/lib/Settings/server-setup";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

function config(opts: { backend?: string; endpoint?: string; ttsEngine?: string; ttsEnabled?: boolean } = {}) {
  return {
    engine: {
      backend: opts.backend ?? "remote-openai",
      whisper_cpp: { model_dir: "", model_size: "base", device: "auto", threads: 0 },
      moonshine: { model_size: "base", language: "en", device: "auto" },
      parakeet: { model_size: "tdt-0.6b-v3", language: "auto", device: "cpu" },
      remote_openai: {
        endpoint: opts.endpoint ?? "http://box:8787/v1",
        api_key: "vx_key",
        model: "",
        language: "",
        timeout_secs: 30,
        fallback: "none",
      },
      s1_mini: { enabled: false, styling: "semi-formal", gpu: true, app_styles: [] },
    },
    features: { custom_vocabulary: [], snippets: {} },
    tts: { enabled: opts.ttsEnabled ?? false, engine: opts.ttsEngine ?? "piper" },
  } as any;
}

let calls: { cmd: string; args: any }[] = [];
let stt: any;
let cleanup: any;
let speech: any;
let pairing: any;
const FINGERPRINT = "0123456789abcdef".repeat(4);

beforeEach(() => {
  calls = [];
  stt = { success: true, message: "Connected to the server.", models: ["parakeet-tdt-0.6b-v3"] };
  cleanup = { available: true, message: "This server cleans up dictation (s1-mini)." };
  speech = { available: true, message: "This server offers 2 speech engines." };
  pairing = { endpoint: "https://192.168.1.162:8787/v1", api_key: "vx_from_code", tls_fingerprint: FINGERPRINT };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    if (cmd === "test_remote_stt") return stt;
    if (cmd === "check_server_cleanup") return cleanup;
    if (cmd === "check_server_tts") return speech;
    if (cmd === "read_pairing_code") {
      if (pairing instanceof Error) throw pairing.message;
      return pairing;
    }
    if (cmd === "get_remote_status") return { state: "online", reason: null, fallback: null };
    if (cmd === "check_s1_mini_downloaded") return false;
    if (cmd === "accelerator_support") return { s1_mini_gpu: null };
    if (cmd === "paste_support") return { supported: true, reason: null };
    return true;
  });
});

describe("server setup rules", () => {
  test("a bare machine name or IP becomes a VoxCtrl server address", () => {
    expect(normalizeServerAddress("192.168.1.162")).toBe("http://192.168.1.162:8787/v1");
    expect(normalizeServerAddress(" mybox ")).toBe("http://mybox:8787/v1");
    expect(normalizeServerAddress("192.168.1.162:9000")).toBe("http://192.168.1.162:9000/v1");
    expect(normalizeServerAddress("")).toBe("");
  });

  test("an address typed in full keeps its port and path, since it may be another service", () => {
    expect(normalizeServerAddress("http://192.168.1.162:8787/v1/")).toBe("http://192.168.1.162:8787/v1");
    expect(normalizeServerAddress("https://api.openai.com/v1")).toBe("https://api.openai.com/v1");
    expect(normalizeServerAddress("https://speech.example.com")).toBe("https://speech.example.com/v1");
    expect(normalizeServerAddress("http://box:8000/v1/audio/transcriptions")).toBe(
      "http://box:8000/v1/audio/transcriptions",
    );
    expect(normalizeServerAddress("mybox/custom/path")).toBe("http://mybox/custom/path");
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

  test("a paired server's address is completed to https", () => {
    expect(normalizeServerAddress("192.168.1.162", true)).toBe("https://192.168.1.162:8787/v1");
    expect(normalizeServerAddress("192.168.1.162", false)).toBe("http://192.168.1.162:8787/v1");
  });

  test("a fingerprint is shown shortened, in the form people compare", () => {
    expect(shortFingerprint(FINGERPRINT)).toBe("01:23:45:67 … 89:AB:CD:EF");
    expect(shortFingerprint("ab01")).toBe("AB:01");
  });

  test("speech counts as on the server only while text-to-speech is enabled", () => {
    expect(speechOnServer(config({ ttsEngine: "server", ttsEnabled: true }))).toBe(true);
    expect(speechOnServer(config({ ttsEngine: "server", ttsEnabled: false }))).toBe(false);
    expect(speechOnServer(config({ ttsEngine: "piper", ttsEnabled: true }))).toBe(false);
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

describe("Engine tab: Remote Speech Engine server setup", () => {
  test("a machine name typed as the endpoint is completed to the server's address", async () => {
    const cfg = config({ endpoint: "" });
    render(EngineTab, { cfg });
    const endpoint = (await screen.findByTestId("remote-endpoint")) as HTMLInputElement;
    await fireEvent.input(endpoint, { target: { value: "192.168.1.162" } });
    await fireEvent.change(endpoint);
    expect(cfg.engine.remote_openai.endpoint).toBe("http://192.168.1.162:8787/v1");
  });

  test("testing the connection reports everything the server offers and takes its model name", async () => {
    const cfg = config();
    render(EngineTab, { cfg });
    await fireEvent.click(await screen.findByText(/Test Connection/));
    await waitFor(() =>
      expect(screen.getByTestId("remote-test-status").textContent).toContain(
        "This server offers speech recognition, dictation cleanup and text-to-speech.",
      ),
    );
    for (const cmd of ["test_remote_stt", "check_server_cleanup", "check_server_tts"]) {
      expect(calls.find((c) => c.cmd === cmd)!.args).toMatchObject({ endpoint: "http://box:8787/v1", apiKey: "vx_key" });
    }
    expect(cfg.engine.remote_openai.model).toBe("parakeet-tdt-0.6b-v3");
  });

  test("a failed connection says why and asks the server nothing more", async () => {
    stt = { success: false, message: "The server rejected the API key (HTTP 401).", models: [] };
    render(EngineTab, { cfg: config() });
    await fireEvent.click(await screen.findByText(/Test Connection/));
    await waitFor(() =>
      expect(screen.getByTestId("remote-test-status").textContent).toContain("rejected the API key"),
    );
    expect(calls.some((c) => c.cmd === "check_server_tts")).toBe(false);
  });

  test("what the server lacks is explained next to its option", async () => {
    cleanup = { available: false, message: "Cleanup is turned off on this server." };
    speech = { available: false, message: "Text-to-speech is turned off on this server." };
    render(EngineTab, { cfg: config() });
    await fireEvent.click(await screen.findByText(/Test Connection/));
    await waitFor(() => expect(screen.getByText("Text-to-speech is turned off on this server.")).toBeTruthy());
    expect(screen.getByText(/Cleanup is turned off on this server\./)).toBeTruthy();
    expect(screen.getByTestId("remote-test-status").textContent).toContain("This server offers speech recognition.");
  });

  test("the text-to-speech option switches the TTS engine to the server and back", async () => {
    const cfg = config({ ttsEngine: "kitten_tts_2", ttsEnabled: false });
    render(EngineTab, { cfg });
    const box = (await screen.findByTestId("speech-on-server")) as HTMLInputElement;
    expect(box.checked).toBe(false);
    await fireEvent.click(box);
    expect(cfg.tts).toMatchObject({ engine: "server", enabled: true });
    await waitFor(() => expect(box.checked).toBe(true));
    await fireEvent.click(box);
    expect(cfg.tts.engine).toBe("kitten_tts_2");
  });

  test("shows text-to-speech already on the server", async () => {
    render(EngineTab, { cfg: config({ ttsEngine: "server", ttsEnabled: true }) });
    expect(((await screen.findByTestId("speech-on-server")) as HTMLInputElement).checked).toBe(true);
  });

  test("the server options belong to the remote backend only", async () => {
    render(EngineTab, { cfg: config({ backend: "parakeet" }) });
    await waitFor(() => expect(screen.getByText("Parakeet Settings")).toBeTruthy());
    expect(screen.queryByTestId("speech-on-server")).toBeNull();
    expect(screen.queryByTestId("remote-endpoint")).toBeNull();
  });
});

describe("Engine tab: pairing with a server", () => {
  test("a pairing code fills in the address, key and certificate, then tests the connection", async () => {
    const cfg = config({ endpoint: "192.168.1.162" });
    render(EngineTab, { cfg });
    const field = (await screen.findByTestId("pairing-code")) as HTMLInputElement;
    expect((screen.getByTestId("pairing-apply") as HTMLButtonElement).disabled).toBe(true);
    await fireEvent.input(field, { target: { value: " vxpair_abc " } });
    await fireEvent.click(screen.getByTestId("pairing-apply"));

    await waitFor(() => expect(screen.getByTestId("pairing-status").textContent).toContain("01:23:45:67"));
    expect(calls.find((c) => c.cmd === "read_pairing_code")!.args).toEqual({
      code: "vxpair_abc",
      typedEndpoint: "192.168.1.162",
    });
    expect(cfg.engine.remote_openai).toMatchObject({
      endpoint: "https://192.168.1.162:8787/v1",
      api_key: "vx_from_code",
      tls_fingerprint: FINGERPRINT,
    });
    // The code holds the key, so it does not stay in the field.
    expect(field.value).toBe("");
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "test_remote_stt")!.args).toMatchObject({
        endpoint: "https://192.168.1.162:8787/v1",
        apiKey: "vx_from_code",
        tlsFingerprint: FINGERPRINT,
      }),
    );
  });

  test("a code from a server without encryption says so and pairs nothing", async () => {
    pairing = { endpoint: "http://box:8787/v1", api_key: "vx_from_code", tls_fingerprint: null };
    const cfg = config();
    render(EngineTab, { cfg });
    await fireEvent.input(await screen.findByTestId("pairing-code"), { target: { value: "vxpair_abc" } });
    await fireEvent.click(screen.getByTestId("pairing-apply"));
    await waitFor(() =>
      expect(screen.getByTestId("pairing-note").textContent).toContain("does not encrypt connections"),
    );
    expect(screen.queryByTestId("pairing-status")).toBeNull();
    expect(cfg.engine.remote_openai.tls_fingerprint).toBeNull();
  });

  test("what is not a pairing code is refused with the reason and changes nothing", async () => {
    pairing = new Error('That is not a pairing code. A pairing code starts with "vxpair_".');
    const cfg = config();
    render(EngineTab, { cfg });
    await fireEvent.input(await screen.findByTestId("pairing-code"), { target: { value: "vx_key" } });
    await fireEvent.click(screen.getByTestId("pairing-apply"));
    await waitFor(() => expect(screen.getByTestId("pairing-note").textContent).toContain("not a pairing code"));
    expect(cfg.engine.remote_openai.endpoint).toBe("http://box:8787/v1");
    expect(calls.some((c) => c.cmd === "test_remote_stt")).toBe(false);
  });

  test("an already paired server shows its certificate, and the pairing can be removed", async () => {
    const cfg = config({ endpoint: "https://box:8787/v1" });
    cfg.engine.remote_openai.tls_fingerprint = FINGERPRINT;
    render(EngineTab, { cfg });
    expect((await screen.findByTestId("pairing-status")).textContent).toContain("Encrypted");
    await fireEvent.click(screen.getByTestId("pairing-remove"));
    expect(cfg.engine.remote_openai.tls_fingerprint).toBeNull();
    await waitFor(() => expect(screen.queryByTestId("pairing-status")).toBeNull());
  });
});

describe("General tab", () => {
  test("no longer carries the server setup", () => {
    const source = readFileSync("src/lib/Settings/GeneralTab.svelte", "utf8");
    expect(source).not.toMatch(/ServerSetupPanel|VoxCtrl Server/);
  });
});
