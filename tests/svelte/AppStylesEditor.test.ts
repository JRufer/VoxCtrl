import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));

import AppStylesEditor from "../../src/lib/Settings/AppStylesEditor.svelte";
import AppStylesHost from "./hosts/AppStylesHost.svelte";
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

  test("removing a row in the middle leaves the others showing their own app and style", async () => {
    const { container } = render(AppStylesHost, {
      props: {
        initial: [
          { app: "slack", styling: "casual" },
          { app: "outlook", styling: "formal" },
          { app: "terminal", styling: "semi-formal" },
        ],
        options: OPTIONS,
      },
    });
    await fireEvent.click(screen.getAllByRole("button", { name: "Remove rule" })[1]);

    const apps = [...container.querySelectorAll<HTMLInputElement>('input[aria-label="App name"]')].map((i) => i.value);
    expect(apps).toEqual(["slack", "terminal"]);
    const rows = [...container.querySelectorAll(".dynamic-list-row")].map((r) => r.textContent ?? "");
    expect(rows[0]).toContain("Casual");
    expect(rows[1]).toContain("Semi-formal");
    expect(rows[1]).not.toContain("Formal (");
    expect(JSON.parse(screen.getByTestId("rules").textContent ?? "")).toEqual([
      { app: "slack", styling: "casual" },
      { app: "terminal", styling: "semi-formal" },
    ]);
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
