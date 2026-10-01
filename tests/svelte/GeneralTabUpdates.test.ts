import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import GeneralTab from "../../src/lib/Settings/GeneralTab.svelte";
import { config } from "../../src/stores/config";
import { get } from "svelte/store";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, args?: unknown) => invoke(cmd, args),
}));

const update = { version: "0.4.0", current_version: "0.3.10", can_self_update: true };

function answer(result: unknown) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "check_for_update") {
      if (result instanceof Error) throw result;
      return result;
    }
    return null;
  });
}

describe("General tab update check", () => {
  beforeEach(() => invoke.mockReset());

  test("there is exactly one update button", () => {
    answer({ current_version: "0.3.10", update: null });
    render(GeneralTab, { cfg: get(config) });
    const buttons = screen.getAllByRole("button").filter((b) => /update|github/i.test(b.textContent ?? ""));
    expect(buttons).toHaveLength(1);
    expect(buttons[0].textContent).toContain("Check for updates");
  });

  test("a newer release opens the update window", async () => {
    answer({ current_version: "0.3.10", update });
    render(GeneralTab, { cfg: get(config) });
    await fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("open_update_window", undefined));
  });

  test("being up to date says so and opens nothing", async () => {
    answer({ current_version: "0.3.10", update: null });
    render(GeneralTab, { cfg: get(config) });
    await fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText(/0\.3\.10 is the latest release/)).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("open_update_window", undefined);
  });

  test("a failed check shows the error", async () => {
    answer(new Error("could not reach GitHub"));
    render(GeneralTab, { cfg: get(config) });
    await fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText(/could not reach GitHub/)).toBeTruthy();
  });
});
