// Key-name helpers shared by every place that records a shortcut: the setup
// wizard, Settings → Hotkeys and the TTS stop key in Settings → TTS. One copy,
// so a combination recorded in one place is the same combination everywhere.

/**
 * Physical keys (by `KeyboardEvent.code`) whose evdev name cannot be derived
 * from the code's spelling. These are exactly the names the backends report
 * for the key — the evdev and X11 backends via the evdev crate, Windows via
 * its scan-code table — and the key matcher compares names exactly, so a
 * recorder that saved anything else would save a shortcut that never fires.
 *
 * Mapped by code rather than by the typed character: the code names the
 * physical key regardless of keyboard layout or Shift, which is also what the
 * backends see. Modifiers keep their side for the same reason.
 */
const CODE_TO_EVDEV: Record<string, string> = {
  ControlLeft: "KEY_LEFTCTRL",
  ControlRight: "KEY_RIGHTCTRL",
  AltLeft: "KEY_LEFTALT",
  AltRight: "KEY_RIGHTALT",
  ShiftLeft: "KEY_LEFTSHIFT",
  ShiftRight: "KEY_RIGHTSHIFT",
  MetaLeft: "KEY_LEFTMETA",
  MetaRight: "KEY_RIGHTMETA",
  OSLeft: "KEY_LEFTMETA",
  OSRight: "KEY_RIGHTMETA",

  Minus: "KEY_MINUS",
  Equal: "KEY_EQUAL",
  BracketLeft: "KEY_LEFTBRACE",
  BracketRight: "KEY_RIGHTBRACE",
  Backslash: "KEY_BACKSLASH",
  Semicolon: "KEY_SEMICOLON",
  Quote: "KEY_APOSTROPHE",
  Backquote: "KEY_GRAVE",
  Comma: "KEY_COMMA",
  Period: "KEY_DOT",
  Slash: "KEY_SLASH",
  IntlBackslash: "KEY_102ND",

  Numpad0: "KEY_KP0",
  Numpad1: "KEY_KP1",
  Numpad2: "KEY_KP2",
  Numpad3: "KEY_KP3",
  Numpad4: "KEY_KP4",
  Numpad5: "KEY_KP5",
  Numpad6: "KEY_KP6",
  Numpad7: "KEY_KP7",
  Numpad8: "KEY_KP8",
  Numpad9: "KEY_KP9",
  NumpadAdd: "KEY_KPPLUS",
  NumpadSubtract: "KEY_KPMINUS",
  NumpadMultiply: "KEY_KPASTERISK",
  NumpadDivide: "KEY_KPSLASH",
  NumpadDecimal: "KEY_KPDOT",
  NumpadEnter: "KEY_KPENTER",
  NumpadEqual: "KEY_KPEQUAL",
};

/**
 * Map a browser key event onto the evdev name VoxCtrl stores in bindings.toml.
 *
 * Every recorder uses this one function: a combination captured in the wizard
 * has to be the same combination when the user later opens Settings → Hotkeys,
 * or the binding they made would appear to have changed by itself.
 */
export function mapBrowserKeyToEvdev(key: string, code: string): string {
  const physical = CODE_TO_EVDEV[code];
  if (physical) return physical;

  const codeUpper = code.toUpperCase();
  if (key === "Control") return "KEY_LEFTCTRL";
  if (key === "Alt") return "KEY_LEFTALT";
  if (key === "Shift") return "KEY_LEFTSHIFT";
  if (key === "Meta" || key === "OS" || key === "Super") return "KEY_LEFTMETA";

  if (codeUpper === "SPACE") return "KEY_SPACE";
  if (codeUpper === "ENTER") return "KEY_ENTER";
  if (codeUpper === "ESCAPE" || codeUpper === "ESC") return "KEY_ESC";
  if (codeUpper === "TAB") return "KEY_TAB";
  if (codeUpper === "BACKSPACE") return "KEY_BACKSPACE";
  if (codeUpper === "DELETE") return "KEY_DELETE";

  if (/^KEY[A-Z]$/.test(codeUpper)) return `KEY_${codeUpper.slice(3)}`;
  if (codeUpper.startsWith("KEY")) return codeUpper;
  if (codeUpper.startsWith("DIGIT")) return `KEY_${codeUpper.replace("DIGIT", "")}`;
  if (codeUpper.startsWith("ARROW")) return `KEY_${codeUpper.replace("ARROW", "")}`;
  if (codeUpper.startsWith("F") && codeUpper.length > 1) return `KEY_${codeUpper}`;

  if (key.length === 1) return `KEY_${key.toUpperCase()}`;
  return `KEY_${codeUpper}`;
}

/** Modifier keys: alone they cannot be a shortcut. */
export const MODIFIER_KEYS = new Set([
  "KEY_LEFTCTRL",
  "KEY_RIGHTCTRL",
  "KEY_LEFTALT",
  "KEY_RIGHTALT",
  "KEY_LEFTSHIFT",
  "KEY_RIGHTSHIFT",
  "KEY_LEFTMETA",
  "KEY_RIGHTMETA",
]);

export function isModifiersOnly(keys: string[]): boolean {
  return keys.length > 0 && keys.every((k) => MODIFIER_KEYS.has(k));
}
