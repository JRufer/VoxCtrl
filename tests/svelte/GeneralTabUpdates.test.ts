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

function answer(result: unknown, autoSupported = true) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "check_for_update") {
      if (result instanceof Error) throw result;
      return result;
    }
    if (cmd === "automatic_update_checks_supported") return autoSupported;
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
    expect(await screen.findByText(/Up to date/)).toBeTruthy();
    expect(invoke).not.toHaveBeenCalledWith("open_update_window", undefined);
  });

  test("a failed check shows the error", async () => {
    answer(new Error("could not reach GitHub"));
    render(GeneralTab, { cfg: get(config) });
    await fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    expect(await screen.findByText(/could not reach GitHub/)).toBeTruthy();
  });
});

describe("General tab automatic update checks", () => {
  beforeEach(() => invoke.mockReset());

  test("are off by default", async () => {
    answer({ current_version: "0.3.10", update: null });
    render(GeneralTab, { cfg: structuredClone(get(config)) });
    const box = (await screen.findByLabelText("Check for updates automatically")) as HTMLInputElement;
    await vi.waitFor(() => expect(box.disabled).toBe(false));
    expect(box.checked).toBe(false);
  });

  test("turning them on marks the setting for saving", async () => {
    answer({ current_version: "0.3.10", update: null });
    const cfg = structuredClone(get(config));
    render(GeneralTab, { cfg });
    const box = (await screen.findByLabelText("Check for updates automatically")) as HTMLInputElement;
    await vi.waitFor(() => expect(box.disabled).toBe(false));
    await fireEvent.click(box);
    expect(get(config).updates.check_automatically).toBe(true);
  });

  test("are not offered on a package-manager install, which says why", async () => {
    answer({ current_version: "0.3.10", update: null }, false);
    render(GeneralTab, { cfg: structuredClone(get(config)) });
    expect(await screen.findByText(/installed by a package\s+manager/)).toBeTruthy();
    expect(screen.queryByLabelText("Check for updates automatically")).toBeNull();
  });
});
