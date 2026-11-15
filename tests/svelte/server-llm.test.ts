import { beforeEach, describe, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import OpenAiTab from "../../src/lib/Settings/OpenAiTab.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

function config(onServer: boolean) {
  return {
    engine: {
      backend: "parakeet",
      remote_openai: { endpoint: "https://box:8787/v1", api_key: "vx_key", tls_fingerprint: "ab".repeat(32) },
    },
    openai: {
      enabled: true,
      endpoint: "http://localhost:11434",
      api_key: null,
      model: "llama3.2:1b",
      mode: "clean",
      custom_prompt: null,
      system_prompt: "Fix grammar.",
      user_prompt: "{text}",
      timeout_secs: 30,
      on_server: onServer,
    },
  } as any;
}

let calls: { cmd: string; args: any }[];
let server: any;

beforeEach(() => {
  calls = [];
  server = { available: true, model: "qwen2.5:7b", message: "This server does AI processing with qwen2.5:7b." };
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (cmd: string, args?: any) => {
    calls.push({ cmd, args });
    if (cmd === "check_server_llm") return server;
    if (cmd === "test_openai") return { success: true, message: "Successfully connected.", models: ["llama3.2:1b"] };
    return true;
  });
});

describe("OpenAI API tab: AI processing through the VoxCtrl server", () => {
  test("off by default: the tab connects to its own service as before", async () => {
    render(OpenAiTab, { cfg: config(false) });
    await waitFor(() => expect(calls.some((c) => c.cmd === "test_openai")).toBe(true));
    expect((screen.getByTestId("openai-on-server") as HTMLInputElement).checked).toBe(false);
    expect(screen.getByText("API URL")).toBeTruthy();
    expect(calls.some((c) => c.cmd === "check_server_llm")).toBe(false);
  });

  test("switching it on asks the server set under Engine and hides the service's own fields", async () => {
    const cfg = config(false);
    render(OpenAiTab, { cfg });
    await fireEvent.click(screen.getByTestId("openai-on-server"));
    expect(cfg.openai.on_server).toBe(true);
    await waitFor(() =>
      expect(screen.getByTestId("openai-test-status").textContent).toContain("AI processing with qwen2.5:7b"),
    );
    expect(calls.find((c) => c.cmd === "check_server_llm")!.args).toEqual({
      endpoint: "https://box:8787/v1",
      apiKey: "vx_key",
      tlsFingerprint: "ab".repeat(32),
    });
    expect(screen.queryByText("API URL")).toBeNull();
    expect(screen.queryByText("API Key")).toBeNull();
    expect(screen.queryByText("Model (Default)")).toBeNull();
    // The prompts are still the user's own.
    expect(screen.getByText("System Prompt")).toBeTruthy();
    // Nothing about the service's own address was thrown away.
    expect(cfg.openai.endpoint).toBe("http://localhost:11434");
  });

  test("a server that does not do it is reported with what to change, and can be asked again", async () => {
    server = { available: false, model: null, message: "AI processing is turned off on this server." };
    render(OpenAiTab, { cfg: config(true) });
    await waitFor(() =>
      expect(screen.getByTestId("openai-test-status").textContent).toContain("turned off on this server"),
    );
    expect(calls.some((c) => c.cmd === "test_openai")).toBe(false);

    server = { available: true, model: "qwen2.5:7b", message: "This server does AI processing with qwen2.5:7b." };
    await fireEvent.click(screen.getByText(/Check the server/));
    await waitFor(() => expect(screen.getByTestId("openai-test-status").textContent).toContain("qwen2.5:7b"));
  });

  test("switching it off goes back to the service's own settings", async () => {
    const cfg = config(true);
    render(OpenAiTab, { cfg });
    await fireEvent.click(await screen.findByTestId("openai-on-server"));
    expect(cfg.openai.on_server).toBe(false);
    await waitFor(() => expect(screen.getByText("API URL")).toBeTruthy());
    await waitFor(() => expect(calls.some((c) => c.cmd === "test_openai")).toBe(true));
  });
});
