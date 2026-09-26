import { describe, test, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import StopKeyRecorder from "../../src/lib/Settings/StopKeyRecorder.svelte";

function renderBound(initial: string[]) {
  const state = { keys: initial };
  const onchange = vi.fn();
  render(StopKeyRecorder, {
    props: {
      get keys() {
        return state.keys;
      },
      set keys(v) {
        state.keys = v;
      },
      onchange,
    },
  });
  return { state, onchange, box: screen.getByRole("button", { name: "Stop key recorder" }) };
}

describe("StopKeyRecorder", () => {
  // Regression: this recorder had its own copy of the key mapper, missing the
  // letter-key rule, so recording Q saved "KEYQ" — a name nothing recognises.
  test("a letter key is recorded under its evdev name", async () => {
    const { state, onchange, box } = renderBound(["KEY_ESC"]);
    await fireEvent.click(box);
    await fireEvent.keyDown(box, { key: "q", code: "KeyQ" });
    await fireEvent.keyUp(box, { key: "q", code: "KeyQ" });
    expect(state.keys).toEqual(["KEY_Q"]);
    expect(onchange).toHaveBeenCalledTimes(1);
  });

  test("a combination is recorded whole, on release", async () => {
    const { state, box } = renderBound([]);
    await fireEvent.focus(box);
    await fireEvent.keyDown(box, { key: "Control", code: "ControlLeft" });
    await fireEvent.keyDown(box, { key: "F5", code: "F5" });
    expect(state.keys).toEqual([]);
    await fireEvent.keyUp(box, { key: "F5", code: "F5" });
    expect(state.keys).toEqual(["KEY_LEFTCTRL", "KEY_F5"]);
  });

  test("Escape alone commits immediately, before the blur it causes", async () => {
    const { state, box } = renderBound(["KEY_Q"]);
    await fireEvent.click(box);
    await fireEvent.keyDown(box, { key: "Escape", code: "Escape" });
    expect(state.keys).toEqual(["KEY_ESC"]);
  });
});
