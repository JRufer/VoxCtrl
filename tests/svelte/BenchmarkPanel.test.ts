import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/svelte";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import BenchmarkPanel from "../../src/lib/Settings/BenchmarkPanel.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));

const steps = [
  { engine: "moonshine", device: "cpu", model: "base", label: "Moonshine (base) on the CPU", est_secs: 3 },
  { engine: "moonshine", device: "gpu", model: "base", label: "Moonshine (base) on the GPU", est_secs: 9 },
  { engine: "s1_mini", device: "cpu", model: "q4_k_m", label: "S1-mini (q4_k_m) on the CPU", est_secs: 3 },
  { engine: "s1_mini", device: "gpu", model: "q4_k_m", label: "S1-mini (q4_k_m) on the GPU", est_secs: 6 },
];
const result = (engine: string, device: string, ms: number) => ({
  engine, device, model: "m", load_ms: 100, median_ms: ms, error: null,
});

beforeEach(() => {
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockClear();
});

describe("BenchmarkPanel", () => {
  test("says how long the test takes and how many tests it runs, before it starts", async () => {
    vi.mocked(invoke).mockResolvedValue({ steps, est_secs: 21 });
    render(BenchmarkPanel, { onApply: vi.fn() });

    expect(await screen.findByText(/takes about 25 seconds for\s+4 tests/)).toBeTruthy();
    expect(screen.getByRole("button", { name: /Test this machine/ })).toBeTruthy();
  });

  test("explains itself instead of offering a test when there is nothing to compare", async () => {
    vi.mocked(invoke).mockResolvedValue({ steps: [], est_secs: 0 });
    render(BenchmarkPanel, { onApply: vi.fn() });

    expect(await screen.findByText(/Nothing to compare/)).toBeTruthy();
    expect(screen.queryByRole("button", { name: /Test this machine/ })).toBeNull();
  });

  test("shows a progress bar while running, driven by the backend's progress events", async () => {
    let finish!: (v: unknown) => void;
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "benchmark_plan") return { steps, est_secs: 21 };
      if (cmd === "run_benchmark") return new Promise((r) => (finish = r));
    });
    render(BenchmarkPanel, { onApply: vi.fn() });
    await fireEvent.click(await screen.findByRole("button", { name: /Test this machine/ }));

    const handler = vi.mocked(listen).mock.calls.find((c) => c[0] === "benchmark-progress")![1] as any;
    handler({ payload: { step: 1, total_steps: 4, label: "Moonshine (base) on the GPU", phase: "Warming up", fraction: 0.35 } });

    const bar = await screen.findByRole("progressbar");
    await waitFor(() => expect(bar.getAttribute("aria-valuenow")).toBe("35"));
    expect(screen.getByText(/Test 2 of 4/)).toBeTruthy();
    expect(screen.getByText(/Warming up/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeTruthy();

    finish({ results: [], recommended: [], cancelled: true });
  });

  test("shows each engine's timings, names the winner, and applies the recommendation", async () => {
    vi.mocked(invoke).mockImplementation(async (cmd: string) => {
      if (cmd === "benchmark_plan") return { steps, est_secs: 21 };
      if (cmd === "run_benchmark")
        return {
          results: [
            result("moonshine", "cpu", 70), result("moonshine", "gpu", 340),
            result("s1_mini", "cpu", 300), result("s1_mini", "gpu", 60),
          ],
          recommended: [["moonshine", "cpu"], ["s1_mini", "gpu"]],
          cancelled: false,
        };
    });
    const onApply = vi.fn();
    render(BenchmarkPanel, { onApply });
    await fireEvent.click(await screen.findByRole("button", { name: /Test this machine/ }));

    expect(await screen.findByText("CPU is 4.9× faster")).toBeTruthy();
    expect(screen.getByText("GPU is 5.0× faster")).toBeTruthy();

    await fireEvent.click(screen.getByRole("button", { name: "Apply fastest settings" }));
    expect(onApply).toHaveBeenCalledWith({ moonshine: "cpu", s1_mini: "gpu" });
  });
});
