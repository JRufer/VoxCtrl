import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, test, expect } from "vitest";
import { MAPPED_CODES, mapBrowserKeyToEvdev } from "../../src/lib/keys";

/**
 * The Windows backend's key vocabulary, read straight out of the Rust source so
 * the two sides cannot drift apart unnoticed. The Linux backends take their
 * names from the evdev crate, and this table spells out that same vocabulary.
 */
function backendKeyNames(): Set<string> {
  // Vitest runs from the repository root (jsdom rewrites import.meta.url).
  const path = resolve(process.cwd(), "crates/voxctrl-hotkeys/src/win_keys.rs");
  const src = readFileSync(path, "utf8");
  const table = /pub const NAMES: &\[&str\] = &\[([\s\S]*?)\];/.exec(src);
  if (!table) throw new Error("keymap::NAMES not found in win_keys.rs");
  const names = new Set([...table[1].matchAll(/"(KEY_[A-Z0-9_]+)"/g)].map((m) => m[1]));
  expect(names.size).toBeGreaterThan(90);
  return names;
}

describe("mapBrowserKeyToEvdev against the backend's key table", () => {
  const known = backendKeyNames();

  test("every code the recorder maps comes out as a name the backend knows", () => {
    const codes = [
      ...MAPPED_CODES,
      ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map((c) => `Key${c}`),
      ..."0123456789".split("").map((d) => `Digit${d}`),
      // F13–F24 are valid evdev names too, but the Windows table stops at F12.
      ...Array.from({ length: 12 }, (_, i) => `F${i + 1}`),
    ];
    const unknown = codes
      .map((code) => [code, mapBrowserKeyToEvdev("", code)] as const)
      .filter(([, name]) => !known.has(name));
    expect(unknown).toEqual([]);
  });

  test("punctuation is named by position, not by the character it types", () => {
    const cases: [string, string, string][] = [
      [".", "Period", "KEY_DOT"],
      [",", "Comma", "KEY_COMMA"],
      ["/", "Slash", "KEY_SLASH"],
      [";", "Semicolon", "KEY_SEMICOLON"],
      ["'", "Quote", "KEY_APOSTROPHE"],
      ["`", "Backquote", "KEY_GRAVE"],
      ["-", "Minus", "KEY_MINUS"],
      ["=", "Equal", "KEY_EQUAL"],
      ["[", "BracketLeft", "KEY_LEFTBRACE"],
      ["]", "BracketRight", "KEY_RIGHTBRACE"],
      ["\\", "Backslash", "KEY_BACKSLASH"],
      ["<", "IntlBackslash", "KEY_102ND"],
    ];
    for (const [key, code, name] of cases) {
      expect(mapBrowserKeyToEvdev(key, code)).toBe(name);
    }
    // Shift changes `key` but not the physical key.
    expect(mapBrowserKeyToEvdev(">", "Period")).toBe("KEY_DOT");
    expect(mapBrowserKeyToEvdev("?", "Slash")).toBe("KEY_SLASH");
    // A non-US layout types something else in the same position.
    expect(mapBrowserKeyToEvdev("ö", "Semicolon")).toBe("KEY_SEMICOLON");
  });

  test("letters follow position, so AZERTY's A key is still KEY_Q", () => {
    expect(mapBrowserKeyToEvdev("a", "KeyQ")).toBe("KEY_Q");
  });

  test("right-hand modifiers keep their side", () => {
    expect(mapBrowserKeyToEvdev("Control", "ControlRight")).toBe("KEY_RIGHTCTRL");
    expect(mapBrowserKeyToEvdev("Alt", "AltRight")).toBe("KEY_RIGHTALT");
    expect(mapBrowserKeyToEvdev("AltGraph", "AltRight")).toBe("KEY_RIGHTALT");
    expect(mapBrowserKeyToEvdev("Shift", "ShiftRight")).toBe("KEY_RIGHTSHIFT");
    expect(mapBrowserKeyToEvdev("Meta", "MetaRight")).toBe("KEY_RIGHTMETA");
    expect(mapBrowserKeyToEvdev("OS", "OSRight")).toBe("KEY_RIGHTMETA");
  });

  test("the keypad is not the top row", () => {
    expect(mapBrowserKeyToEvdev("1", "Numpad1")).toBe("KEY_KP1");
    // NumLock off: `key` becomes a navigation key, the physical key is the same.
    expect(mapBrowserKeyToEvdev("End", "Numpad1")).toBe("KEY_KP1");
    expect(mapBrowserKeyToEvdev("Enter", "NumpadEnter")).toBe("KEY_KPENTER");
    expect(mapBrowserKeyToEvdev(".", "NumpadDecimal")).toBe("KEY_KPDOT");
    expect(mapBrowserKeyToEvdev("+", "NumpadAdd")).toBe("KEY_KPPLUS");
    expect(mapBrowserKeyToEvdev("*", "NumpadMultiply")).toBe("KEY_KPASTERISK");
  });

  test("keys whose DOM name differs from evdev's use evdev's", () => {
    expect(mapBrowserKeyToEvdev("PrintScreen", "PrintScreen")).toBe("KEY_SYSRQ");
    expect(mapBrowserKeyToEvdev("ContextMenu", "ContextMenu")).toBe("KEY_COMPOSE");
    expect(mapBrowserKeyToEvdev("PageUp", "PageUp")).toBe("KEY_PAGEUP");
  });

  test("an event with no code still names modifiers, letters and digits", () => {
    expect(mapBrowserKeyToEvdev("Control", "")).toBe("KEY_LEFTCTRL");
    expect(mapBrowserKeyToEvdev("v", "")).toBe("KEY_V");
    expect(mapBrowserKeyToEvdev("7", "")).toBe("KEY_7");
  });
});
