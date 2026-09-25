import { describe, test, expect } from "vitest";
import {
  bindingSignature,
  bindingTargetsLabel,
  duplicateSignatures,
  sameKeys,
  targetOptionLabel,
} from "../../src/lib/Settings/hotkeys";
import { newOutputTarget } from "../../src/lib/Settings/routing-types";
import type { HotkeyBinding, OutputTarget } from "../../src/lib/Settings/routing-types";

function binding(id: string, keys: string[], extra: Partial<HotkeyBinding> = {}): HotkeyBinding {
  return {
    id,
    label: id,
    keys,
    gesture: "hold",
    target_id: "default",
    target_ids: ["default"],
    tap_ms: 300,
    hold_threshold_ms: 200,
    disabled: false,
    ...extra,
  } as HotkeyBinding;
}

describe("hotkey conflict detection", () => {
  test("a signature ignores key order but not the gesture", () => {
    expect(bindingSignature(["KEY_B", "KEY_A"], "hold")).toBe(bindingSignature(["KEY_A", "KEY_B"], "hold"));
    expect(bindingSignature(["KEY_A"], "hold")).not.toBe(bindingSignature(["KEY_A"], "toggle"));
  });

  test("shared combinations are reported, disabled ones only when asked", () => {
    const bindings = [
      binding("a", ["KEY_LEFTCTRL", "KEY_F9"]),
      binding("b", ["KEY_F9", "KEY_LEFTCTRL"], { disabled: true }),
      binding("c", ["KEY_F10"]),
      binding("empty", []),
      binding("empty2", []),
    ];
    const sig = bindingSignature(["KEY_LEFTCTRL", "KEY_F9"], "hold");
    expect(duplicateSignatures(bindings)).toEqual(new Set([sig]));
    expect(duplicateSignatures(bindings, true)).toEqual(new Set());
  });

  test("sameKeys compares combinations in any order", () => {
    expect(sameKeys(["KEY_A", "KEY_B"], ["KEY_B", "KEY_A"])).toBe(true);
    expect(sameKeys(["KEY_A"], ["KEY_A", "KEY_B"])).toBe(false);
  });
});

describe("target labels", () => {
  const targets = [{ ...newOutputTarget(), id: "notes", label: "Notes", delivery: "file" }] as OutputTarget[];

  test("binding targets list labels, with the default target named", () => {
    expect(bindingTargetsLabel(binding("x", [], { target_ids: ["notes", "default", "gone"] }), targets)).toBe(
      "Notes, Focused Window, gone",
    );
    expect(bindingTargetsLabel(binding("x", [], { target_ids: [], target_id: "notes" }), targets)).toBe("Notes");
  });

  test("picker labels include the delivery type", () => {
    expect(targetOptionLabel("notes", targets)).toBe("Notes (file)");
    expect(targetOptionLabel("default", targets)).toBe("Focused Window");
  });

  test("new targets get fresh ids", () => {
    expect(newOutputTarget().id).toMatch(/^new_target_/);
    expect(newOutputTarget().id).not.toBe(newOutputTarget().id);
  });
});
