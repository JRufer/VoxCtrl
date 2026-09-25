// ── Key naming ───────────────────────────────────────────────────────────────
//
// Every shortcut recorder (setup wizard, Settings → Hotkeys, the TTS stop key)
// goes through `mapBrowserKeyToEvdev`, so a combination captured in one place is
// the same combination everywhere else and the hotkey backends can match it.
//
// The names are the Linux evdev ones the Rust side speaks: the evdev and X11
// backends take them from the evdev crate, and the Windows backend's table
// (`crates/voxctrl-hotkeys/src/win_keys.rs`, `keymap::NAMES`) spells out the
// same vocabulary. `tests/svelte/keys.test.ts` checks every name emitted here
// against that table.

/**
 * `KeyboardEvent.code` → evdev name, for every key that is not a letter, digit
 * or function key.
 *
 * Keyed by `code` rather than `key` because `code` is the physical position and
 * so is what the backends see: `key` changes with layout and Shift (`.` vs `>`),
 * and cannot tell left Ctrl from right Ctrl or the keypad from the top row.
 */
const CODE_TO_EVDEV: Record<string, string> = {
  // Modifiers keep their side, as every backend reports them.
  ControlLeft: "KEY_LEFTCTRL",
  ControlRight: "KEY_RIGHTCTRL",
  AltLeft: "KEY_LEFTALT",
  AltRight: "KEY_RIGHTALT",
  ShiftLeft: "KEY_LEFTSHIFT",
  ShiftRight: "KEY_RIGHTSHIFT",
  MetaLeft: "KEY_LEFTMETA",
  MetaRight: "KEY_RIGHTMETA",
  // Older WebKit/Firefox spelling of the Super keys.
  OSLeft: "KEY_LEFTMETA",
  OSRight: "KEY_RIGHTMETA",

  Space: "KEY_SPACE",
  Enter: "KEY_ENTER",
  Escape: "KEY_ESC",
  Tab: "KEY_TAB",
  Backspace: "KEY_BACKSPACE",
  Delete: "KEY_DELETE",
  Insert: "KEY_INSERT",
  Home: "KEY_HOME",
  End: "KEY_END",
  PageUp: "KEY_PAGEUP",
  PageDown: "KEY_PAGEDOWN",
  ArrowUp: "KEY_UP",
  ArrowDown: "KEY_DOWN",
  ArrowLeft: "KEY_LEFT",
  ArrowRight: "KEY_RIGHT",
  CapsLock: "KEY_CAPSLOCK",
  NumLock: "KEY_NUMLOCK",
  ScrollLock: "KEY_SCROLLLOCK",
  Pause: "KEY_PAUSE",
  PrintScreen: "KEY_SYSRQ",
  ContextMenu: "KEY_COMPOSE",

  // Punctuation, named after the US-layout key in that position.
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
  // The extra key ISO keyboards carry beside left Shift.
  IntlBackslash: "KEY_102ND",

  // Keypad. Distinct from the top row: evdev reports KEY_KP1, not KEY_1, and
  // does so whether or not NumLock is on.
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
  NumpadDecimal: "KEY_KPDOT",
  NumpadEnter: "KEY_KPENTER",
  NumpadAdd: "KEY_KPPLUS",
  NumpadSubtract: "KEY_KPMINUS",
  NumpadMultiply: "KEY_KPASTERISK",
  NumpadDivide: "KEY_KPSLASH",

  // Media keys, when the webview delivers them at all.
  AudioVolumeMute: "KEY_MUTE",
  AudioVolumeDown: "KEY_VOLUMEDOWN",
  AudioVolumeUp: "KEY_VOLUMEUP",
  MediaTrackNext: "KEY_NEXTSONG",
  MediaTrackPrevious: "KEY_PREVIOUSSONG",
  MediaStop: "KEY_STOPCD",
  MediaPlayPause: "KEY_PLAYPAUSE",
};

/** The `code` values `mapBrowserKeyToEvdev` names from its table. */
export const MAPPED_CODES: readonly string[] = Object.keys(CODE_TO_EVDEV);

/** Map a browser key event onto the evdev name VoxCtrl stores in its config. */
export function mapBrowserKeyToEvdev(key: string, code: string): string {
  const mapped = CODE_TO_EVDEV[code];
  if (mapped) return mapped;

  const letter = /^Key([A-Z])$/.exec(code);
  if (letter) return `KEY_${letter[1]}`;
  const digit = /^Digit([0-9])$/.exec(code);
  if (digit) return `KEY_${digit[1]}`;
  const fn = /^F([1-9]|1[0-9]|2[0-4])$/.exec(code);
  if (fn) return `KEY_F${fn[1]}`;

  // No usable `code` (some synthetic or IME events leave it empty): fall back
  // on `key`, which is still unambiguous for modifiers, letters and digits.
  if (key === "Control") return "KEY_LEFTCTRL";
  if (key === "Alt") return "KEY_LEFTALT";
  if (key === "Shift") return "KEY_LEFTSHIFT";
  if (key === "Meta" || key === "OS" || key === "Super") return "KEY_LEFTMETA";
  if (/^[a-zA-Z0-9]$/.test(key)) return `KEY_${key.toUpperCase()}`;

  return `KEY_${code.toUpperCase()}`;
}
