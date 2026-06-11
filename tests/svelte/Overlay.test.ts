import { describe, test, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/svelte";

// Mock tauri IPC used by the overlay components and the status store
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => ({
    recording: false,
    processing: false,
    speaking: false,
    mcp_recording: false,
    audio_ready: true,
    word_count: 0,
    active_target_id: "default",
    active_target_label: "Focused Window",
  })),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => {
    return () => {};
  }),
}));

// The components run requestAnimationFrame render loops on mount
vi.stubGlobal(
  "requestAnimationFrame",
  (cb: FrameRequestCallback) => setTimeout(() => cb(performance.now()), 16) as unknown as number
);
vi.stubGlobal("cancelAnimationFrame", (id: number) => clearTimeout(id));

import { status, type AppStatus } from "../../src/stores/status";
import Waveform from "../../src/lib/Overlay/Waveform.svelte";
import Pulse from "../../src/lib/Overlay/Pulse.svelte";
import BlueWave from "../../src/lib/Overlay/BlueWave.svelte";
import VoiceCard from "../../src/lib/Overlay/VoiceCard.svelte";

function setStatus(partial: Partial<AppStatus> = {}) {
  status.set({
    recording: false,
    processing: false,
    speaking: false,
    mcp_recording: false,
    audio_ready: true,
    word_count: 0,
    active_target_id: "default",
    active_target_label: "Focused Window",
    ...partial,
  });
}

beforeEach(() => {
  setStatus();
});

describe("Waveform.svelte (oscilloscope)", () => {
  test("shows the scope title and the active target readout", () => {
    setStatus({ recording: true, active_target_label: "Kitty Terminal" });
    const { container } = render(Waveform, { recording: true, active: true });

    expect(container.textContent).toContain("WAVEFORM // OSC-01");
    expect(container.textContent).toContain("Kitty Terminal");
    expect(container.textContent).toContain("LIVE TRACE");
  });

  test("plays the CRT load/unload animation via the active prop", () => {
    const { container: onContainer } = render(Waveform, { recording: true, active: true });
    expect(onContainer.querySelector(".scope")?.classList.contains("on")).toBe(true);

    const { container: offContainer } = render(Waveform, { recording: true, active: false });
    expect(offContainer.querySelector(".scope")?.classList.contains("on")).toBe(false);
  });

  test("switches to the transcribing state while processing", () => {
    setStatus({ processing: true });
    const { container } = render(Waveform, { recording: false, active: true });
    expect(container.textContent).toContain("TRANSCRIBING");
  });

  test("shows the calibrating state while audio is initializing", () => {
    setStatus({ recording: true, audio_ready: false });
    const { container } = render(Waveform, { recording: true, active: true });
    expect(container.textContent).toContain("CALIBRATING");
  });
});

describe("Pulse.svelte (radar)", () => {
  test("locks onto the active target", () => {
    setStatus({ recording: true, active_target_label: "Firefox Browser" });
    const { container } = render(Pulse, { recording: true, active: true });

    expect(container.textContent).toContain("PULSE // TARGET LOCK");
    expect(container.textContent).toContain("Firefox Browser");
  });

  test("renders the dial with sweep, rings, blips and core", () => {
    const { container } = render(Pulse, { recording: true, active: true });
    expect(container.querySelector(".dial .sweep")).not.toBeNull();
    expect(container.querySelectorAll(".dial .pulse-ring").length).toBe(2);
    expect(container.querySelectorAll(".dial .blip").length).toBe(2);
    expect(container.querySelector(".dial .core")).not.toBeNull();
  });

  test("plays the load/unload animation via the active prop", () => {
    const { container } = render(Pulse, { recording: true, active: false });
    expect(container.querySelector(".radar")?.classList.contains("on")).toBe(false);
  });

  test("shows acquiring and analyzing states", () => {
    setStatus({ recording: true, audio_ready: false });
    const { container: initContainer } = render(Pulse, { recording: true, active: true });
    expect(initContainer.textContent).toContain("PULSE // ACQUIRING");
    expect(initContainer.textContent).toContain("Connecting mic…");

    setStatus({ processing: true });
    const { container: procContainer } = render(Pulse, { recording: false, active: true });
    expect(procContainer.textContent).toContain("PULSE // ANALYZING");
    expect(procContainer.textContent).toContain("Decoding transmission…");
  });
});

describe("BlueWave.svelte (ocean)", () => {
  test("shows the title and floats the target on the buoy", () => {
    setStatus({ recording: true, active_target_label: "Code Editor" });
    const { container } = render(BlueWave, { recording: true, active: true });

    expect(container.textContent).toContain("OCEAN WAVE");
    expect(container.textContent).toContain("high tide — listening");
    expect(container.querySelector(".buoy")?.textContent).toContain("Code Editor");
  });

  test("water fills on load and drains on unload via the active prop", () => {
    const { container: onContainer } = render(BlueWave, { recording: true, active: true });
    expect(onContainer.querySelector(".ocean")?.classList.contains("on")).toBe(true);

    const { container: offContainer } = render(BlueWave, { recording: true, active: false });
    expect(offContainer.querySelector(".ocean")?.classList.contains("on")).toBe(false);
  });

  test("shows tide states for initializing and processing", () => {
    setStatus({ recording: true, audio_ready: false });
    const { container: initContainer } = render(BlueWave, { recording: true, active: true });
    expect(initContainer.textContent).toContain("low tide — preparing");
    expect(initContainer.querySelector(".buoy")?.textContent).toContain("Casting off…");

    setStatus({ processing: true });
    const { container: procContainer } = render(BlueWave, { recording: false, active: true });
    expect(procContainer.textContent).toContain("deep current — processing");
    expect(procContainer.querySelector(".buoy")?.textContent).toContain("Sounding the depths…");
  });
});

describe("VoiceCard.svelte (membership card)", () => {
  test("embosses the brand, REC stamp and target field", () => {
    setStatus({ recording: true, active_target_label: "Slack" });
    const { container } = render(VoiceCard, { recording: true, active: true });

    expect(container.textContent).toContain("VOXCTRL");
    expect(container.textContent).toContain("VOICE CARD");
    expect(container.textContent).toContain("REC");
    expect(container.textContent).toContain("TARGET");
    expect(container.querySelector(".field-value")?.textContent).toContain("Slack");
  });

  test("renders the full 20x6 VU LED dot matrix", () => {
    const { container } = render(VoiceCard, { recording: true, active: true });
    expect(container.querySelectorAll(".matrix .dot").length).toBe(120);
    // VU colouring: one red top row, two amber rows, three green rows per column
    expect(container.querySelectorAll(".matrix .dot.red").length).toBe(20);
    expect(container.querySelectorAll(".matrix .dot.amber").length).toBe(40);
    expect(container.querySelectorAll(".matrix .dot.green").length).toBe(60);
  });

  test("card-flip load/unload animation via the active prop", () => {
    const { container: onContainer } = render(VoiceCard, { recording: true, active: true });
    expect(onContainer.querySelector(".card-scene")?.classList.contains("on")).toBe(true);

    const { container: offContainer } = render(VoiceCard, { recording: true, active: false });
    expect(offContainer.querySelector(".card-scene")?.classList.contains("on")).toBe(false);
  });

  test("stamps INIT while initializing and PROC while processing", () => {
    setStatus({ recording: true, audio_ready: false });
    const { container: initContainer } = render(VoiceCard, { recording: true, active: true });
    expect(initContainer.querySelector(".stamp")?.textContent).toContain("INIT");

    setStatus({ processing: true });
    const { container: procContainer } = render(VoiceCard, { recording: false, active: true });
    expect(procContainer.querySelector(".stamp")?.textContent).toContain("PROC");
    expect(procContainer.querySelector(".field-value")?.textContent).toContain("Reading the card…");
  });
});
