import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import KeyValueListEditor from "../../src/lib/Settings/KeyValueListEditor.svelte";

/**
 * Render the editor with `value` bound the way `bind:value` binds it: through
 * accessors on the props object, so every write the component makes is seen.
 */
function renderBound(initial: Record<string, string> | undefined) {
  const state = { value: initial };
  const onchange = vi.fn();
  const props = {
    get value() {
      return state.value;
    },
    set value(v) {
      state.value = v;
    },
    onchange,
    title: "Snippets",
    hint: "Type a trigger word.",
    addLabel: "Add Snippet",
    keyPlaceholder: "Trigger word",
    valuePlaceholder: "Expansion text",
    emptyText: "No snippets defined.",
  };
  render(KeyValueListEditor, { props });
  return { state, onchange };
}

describe("KeyValueListEditor", () => {
  test("shows the saved pairs, and does not count loading them as an edit", () => {
    const { onchange } = renderBound({ brb: "be right back" });
    expect((screen.getByPlaceholderText("Trigger word") as HTMLInputElement).value).toBe("brb");
    expect((screen.getByPlaceholderText("Expansion text") as HTMLInputElement).value).toBe("be right back");
    expect(onchange).not.toHaveBeenCalled();
  });

  test("an added row is written back trimmed, and marks the config changed", async () => {
    const { state, onchange } = renderBound({});
    expect(screen.getByText("No snippets defined.")).toBeTruthy();

    await fireEvent.click(screen.getByText(/Add Snippet/));
    await fireEvent.input(screen.getByPlaceholderText("Trigger word"), { target: { value: " omw " } });
    await fireEvent.input(screen.getByPlaceholderText("Expansion text"), { target: { value: " on my way " } });

    expect(state.value).toEqual({ omw: "on my way" });
    expect(onchange).toHaveBeenCalled();
  });

  test("a row with a blank key is not saved, and removing a row drops it", async () => {
    const { state } = renderBound({ a: "1", b: "2" });
    const keys = screen.getAllByPlaceholderText("Trigger word");
    await fireEvent.input(keys[0], { target: { value: "  " } });
    expect(state.value).toEqual({ b: "2" });

    await fireEvent.click(screen.getAllByText("✕")[1]);
    expect(state.value).toEqual({});
  });
});
