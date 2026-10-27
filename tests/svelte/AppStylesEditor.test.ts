import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import AppStylesEditor from "../../src/lib/Settings/AppStylesEditor.svelte";
import type { AppStyleRule } from "../../src/stores/config";

const OPTIONS = [
  { value: "semi-formal", label: "Semi-formal (Default)" },
  { value: "casual", label: "Casual" },
  { value: "formal", label: "Formal" },
];

function renderBound(initial: AppStyleRule[]) {
  const state = { rules: initial };
  const onchange = vi.fn();
  render(AppStylesEditor, {
    props: {
      get rules() {
        return state.rules;
      },
      set rules(v) {
        state.rules = v;
      },
      styleOptions: OPTIONS,
      onchange,
    },
  });
  return { state, onchange };
}

beforeEach(() => {
  invokeMock.mockReset();
});

describe("AppStylesEditor", () => {
  test("shows an empty state when there are no rules", () => {
    renderBound([]);
    expect(screen.getByText(/No per-app styling yet/)).toBeTruthy();
  });

  test("detect adds a rule for the app that was focused", async () => {
    invokeMock.mockResolvedValue("Slack");
    const { state, onchange } = renderBound([]);
    await fireEvent.click(screen.getByRole("button", { name: /Detect app/ }));
    await vi.waitFor(() => expect(state.rules).toEqual([{ app: "Slack", styling: "casual" }]));
    expect(invokeMock).toHaveBeenCalledWith("detect_focused_app", { delayMs: 3000 });
    expect(onchange).toHaveBeenCalled();
    expect(screen.getByRole("status").textContent).toContain("Slack");
  });

  test("detect doesn't duplicate an app that already has a rule", async () => {
    invokeMock.mockResolvedValue("slack");
    const { state } = renderBound([{ app: "Slack", styling: "formal" }]);
    await fireEvent.click(screen.getByRole("button", { name: /Detect app/ }));
    await vi.waitFor(() => expect(screen.getByRole("status").textContent).toContain("slack"));
    expect(state.rules).toEqual([{ app: "Slack", styling: "formal" }]);
  });

  test("an undetectable app explains why and adds nothing", async () => {
    invokeMock.mockResolvedValue(null);
    const { state } = renderBound([]);
    await fireEvent.click(screen.getByRole("button", { name: /Detect app/ }));
    await vi.waitFor(() => expect(screen.getByRole("status").textContent).toContain("kdotool"));
    expect(state.rules).toEqual([]);
  });

  test("rules can be added and removed by hand", async () => {
    const { state, onchange } = renderBound([{ app: "thunderbird", styling: "formal" }]);
    await fireEvent.click(screen.getByRole("button", { name: /Add app/ }));
    expect(state.rules).toHaveLength(2);
    await fireEvent.click(screen.getAllByRole("button", { name: "Remove rule" })[0]);
    expect(state.rules).toEqual([{ app: "", styling: "casual" }]);
    expect(onchange).toHaveBeenCalledTimes(2);
  });
});
