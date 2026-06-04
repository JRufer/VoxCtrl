import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import RoutingTab from "../../src/lib/Settings/RoutingTab.svelte";
import type { HotkeyBinding, OutputTarget } from "../../src/lib/Settings/routing-types";

let mockBindings: HotkeyBinding[] = [];
let mockTargets: OutputTarget[] = [];

// Mock tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (cmd) => {
    if (cmd === "get_targets") {
      return mockTargets;
    }
    if (cmd === "get_bindings") {
      return mockBindings;
    }
    return {};
  }),
}));

describe("RoutingTab.svelte Conflict Detection", () => {
  beforeEach(() => {
    mockTargets = [
      {
        id: "default",
        label: "Focused Window",
        delivery: "inject",
        file_prefix: "",
        file_timestamp: true,
        send_on_release: true,
        append_newline: false,
        strip_newlines: false,
        tts_engine: "None",
      },
    ];
    mockBindings = [];
  });

  async function switchToBindingsTab() {
    const tabButton = await screen.findByText(/Hotkey Bindings/i);
    await fireEvent.click(tabButton);
  }

  test("does not show conflict warnings when there are no conflicts", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
      {
        id: "bind2",
        keys: ["KEY_LEFTMETA", "KEY_ENTER"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 2",
        disabled: false,
      },
    ];

    render(RoutingTab);
    await switchToBindingsTab();

    // Conflict banner should NOT be present
    const banner = screen.queryByText(/Conflict detected/i);
    expect(banner).toBeNull();

    // No CONFLICT markers should be present
    const marker = screen.queryByText("CONFLICT");
    expect(marker).toBeNull();
  });

  test("shows active conflicts with yellow background and CONFLICT marker when both are enabled", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
      {
        id: "bind2",
        keys: ["KEY_SPACE", "KEY_LEFTMETA"], // Same keys, different order
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 2",
        disabled: false,
      },
    ];

    const { container } = render(RoutingTab);
    await switchToBindingsTab();

    // Conflict banner should be present
    const banner = await screen.findByText(/Conflict detected/i);
    expect(banner).not.toBeNull();

    // CONFLICT markers should be rendered
    const markers = await screen.findAllByText("CONFLICT");
    expect(markers.length).toBe(2);

    // The cards should have active-conflict class
    const conflictItems = container.querySelectorAll(".active-conflict");
    expect(conflictItems.length).toBe(2);
  });

  test("shows conflict borders but no CONFLICT markers or active-conflict background when one is disabled", async () => {
    mockBindings = [
      {
        id: "bind1",
        keys: ["KEY_LEFTMETA", "KEY_SPACE"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 1",
        disabled: false,
      },
      {
        id: "bind2",
        keys: ["KEY_SPACE", "KEY_LEFTMETA"],
        gesture: "hold",
        target_id: "default",
        target_ids: ["default"],
        tap_ms: 300,
        hold_threshold_ms: 1000,
        label: "Binding 2",
        disabled: true, // One is disabled
      },
    ];

    const { container } = render(RoutingTab);
    await switchToBindingsTab();

    // Conflict banner should still be present because a conflict exists in the list
    const banner = await screen.findByText(/Conflict detected/i);
    expect(banner).not.toBeNull();

    // No CONFLICT markers should be rendered (since one is disabled, the active one works)
    const marker = screen.queryByText("CONFLICT");
    expect(marker).toBeNull();

    // The cards should have has-conflict class but NOT active-conflict class
    const hasConflictItems = container.querySelectorAll(".has-conflict");
    expect(hasConflictItems.length).toBe(2);

    const activeConflictItems = container.querySelectorAll(".active-conflict");
    expect(activeConflictItems.length).toBe(0);
  });
});
