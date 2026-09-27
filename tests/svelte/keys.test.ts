import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, test, expect } from "vitest";
import { MAPPED_CODES, mapBrowserKeyToEvdev } from "../../src/lib/keys";
import { keycapLabel } from "../../src/lib/Wizard/wizard-data";

/**
 * The backends' key vocabulary, read out of `keymap::NAMES` in the Rust source
 * so the recorder and the backends cannot drift apart unnoticed. The Linux
 * backends take their names from the evdev crate, and this table spells out
 * the same names for Windows.
 */
function backendKeyNames(): Set<string> {
  // Vitest runs from the repository root.
  const path = resolve(process.cwd(), "crates/voxctrl-hotkeys/src/win_keys.rs");
  const src = readFileSync(path, "utf8");
  const table = /pub const NAMES: &\[&str\] = &\[([\s\S]*?)\];/.exec(src);
  if (!table) throw new Error("keymap::NAMES not found in win_keys.rs");
  return new Set([...table[1].matchAll(/"(KEY_[A-Z0-9_]+)"/g)].map((m) => m[1]));
}

/**
 * Valid evdev names the Windows table has no scan code for. They work on
 * Linux, which is where the keys exist in practice.
 */
const LINUX_ONLY = new Set(["KEY_KPEQUAL"]);

describe("mapBrowserKeyToEvdev against the backends' key table", () => {
  const known = backendKeyNames();

  test("the table was read", () => {
    expect(known.size).toBeGreaterThan(90);
  });

  test("every key the recorder can name is one a backend reports", () => {
    const codes = [
      ...MAPPED_CODES,
      ...[..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"].map((c) => `Key${c}`),
      ...[..."0123456789"].map((d) => `Digit${d}`),
      // F13–F24 are valid evdev names too, but the Windows table stops at F12.
      ...Array.from({ length: 12 }, (_, i) => `F${i + 1}`),
      "Space",
      "Enter",
      "Escape",
      "Tab",
      "Backspace",
      "Delete",
      "Insert",
      "Home",
      "End",
      "PageUp",
      "PageDown",
      "ArrowUp",
      "ArrowDown",
      "ArrowLeft",
      "ArrowRight",
      "CapsLock",
      "NumLock",
      "ScrollLock",
      "Pause",
      "PrintScreen",
      "ContextMenu",
      "AudioVolumeMute",
      "MediaPlayPause",
    ];
    const unknown = codes
      .map((code) => [code, mapBrowserKeyToEvdev("", code)] as const)
      .filter(([, name]) => !known.has(name) && !LINUX_ONLY.has(name));
    expect(unknown).toEqual([]);
  });

  test("keys whose DOM name differs from evdev's use evdev's", () => {
    expect(mapBrowserKeyToEvdev("PrintScreen", "PrintScreen")).toBe("KEY_SYSRQ");
    expect(mapBrowserKeyToEvdev("ContextMenu", "ContextMenu")).toBe("KEY_COMPOSE");
    expect(mapBrowserKeyToEvdev("AudioVolumeMute", "AudioVolumeMute")).toBe("KEY_MUTE");
    expect(mapBrowserKeyToEvdev("MediaPlayPause", "MediaPlayPause")).toBe("KEY_PLAYPAUSE");
  });

  test("every mapped key has a readable keycap", () => {
    for (const code of MAPPED_CODES) {
      const label = keycapLabel(mapBrowserKeyToEvdev("", code));
      expect(label, code).not.toMatch(/^(KEY_|KP|AUDIO|MEDIA)/);
    }
  });
});
