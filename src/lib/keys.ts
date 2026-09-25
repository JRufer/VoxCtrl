// Key-name helpers shared by every place that records a shortcut: the setup
// wizard, Settings → Hotkeys and the TTS stop key in Settings → TTS. One copy,
// so a combination recorded in one place is the same combination everywhere.

/**
 * Map a browser key event onto the evdev name VoxCtrl stores in bindings.toml.
 *
 * Every recorder uses this one function: a combination captured in the wizard
 * has to be the same combination when the user later opens Settings → Hotkeys,
 * or the binding they made would appear to have changed by itself.
 */
export function mapBrowserKeyToEvdev(key: string, code: string): string {
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
