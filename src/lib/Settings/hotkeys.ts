// Types and pure logic for Settings → Hotkeys, kept out of the component so it
// can be read and tested on its own.

import type { HotkeyBinding, OutputTarget } from "./routing-types";

// How shortcuts are actually reaching the app. Shown in the UI because the
// answer decides what VoxCtrl can see: on the portal it is told only that its
// own shortcut fired, and the keys are chosen and owned by the desktop.
export type BoundShortcut = {
  binding_ids: string[];
  requested: string | null;
  trigger_description: string;
  bound: boolean;
};

export type GestureType = "hold" | "toggle" | "double_tap" | "double_tap_hold";

export const GESTURE_LABELS: Record<GestureType, string> = {
  hold: "Hold keys to dictate (Release to transcribe)",
  toggle: "Tap once to start recording, tap again to finish",
  double_tap: "Double-tap hotkey to trigger recording",
  double_tap_hold: "Double-tap & hold keys to dictate (Release to transcribe)",
};

export const GESTURE_ORDER: GestureType[] = ["hold", "toggle", "double_tap", "double_tap_hold"];

export type HotkeyStatus = {
  is_active: boolean;
  backend: string;
  is_private: boolean;
  portal_error: string | null;
  portal_refused: boolean;
  shortcuts: BoundShortcut[];
  // Gesture styles the running backend can actually deliver. A backend that
  // only learns about key presses (a Cinnamon/MATE native shortcut) cannot
  // end a hold or tell a tap from a hold, so it reports "toggle" alone.
  supported_gestures: GestureType[];
  x11_error: string | null;
  session_type: string;
  devices_total: number;
  devices_readable: number;
  needs_attention: boolean;
  detail: string;
  // KDE registers portal shortcuts disabled and gives no way to check that
  // over D-Bus, so this is a standing warning on KDE rather than a detected
  // fact about any one shortcut. See docs/hotkeys.md.
  needs_manual_enable: boolean;
  manual_enable_hint: string | null;
};

// Result of validating a captured combination. The rules live in Rust
// (`voxctrl_hotkeys::accelerator`) and are reached over IPC, so the recorder
// and the portal registration cannot disagree about what is bindable.
export type KeysCheck = {
  accepted: boolean;
  enforced: boolean;
  accelerator: string | null;
  problem: string | null;
  message: string | null;
};

/** Canonical signature for a binding's key combination and gesture type. */
export function bindingSignature(keys: string[], gesture: string): string {
  const sortedKeys = [...keys].sort().join(",");
  return `${gesture}:${sortedKeys}`;
}

/** Whether two key lists are the same combination, in any order. */
export function sameKeys(a: string[], b: string[]): boolean {
  return [...a].sort().join(",") === [...b].sort().join(",");
}

/**
 * Signatures shared by two or more bindings. With `activeOnly`, disabled
 * bindings are left out, since they cannot collide at runtime.
 */
export function duplicateSignatures(bindings: HotkeyBinding[], activeOnly = false): Set<string> {
  const counts = new Map<string, number>();
  for (const b of bindings) {
    if ((activeOnly && b.disabled) || !b.keys || b.keys.length === 0) continue;
    const sig = bindingSignature(b.keys, b.gesture);
    counts.set(sig, (counts.get(sig) || 0) + 1);
  }
  return new Set([...counts].filter(([, count]) => count > 1).map(([sig]) => sig));
}

/** The label of target `id`, as the binding list shows it. */
export function targetName(id: string, targets: OutputTarget[]): string {
  const t = targets.find((target) => target.id === id);
  return t ? t.label : id === "default" ? "Focused Window" : id;
}

/** Comma-separated labels of every target a binding delivers to. */
export function bindingTargetsLabel(b: HotkeyBinding, targets: OutputTarget[]): string {
  const ids = b.target_ids && b.target_ids.length > 0 ? b.target_ids : [b.target_id];
  return ids.map((id) => targetName(id, targets)).join(", ");
}

/** A target's label with its delivery type, for the target pickers. */
export function targetOptionLabel(id: string, targets: OutputTarget[]): string {
  const t = targets.find((target) => target.id === id);
  return t ? `${t.label} (${t.delivery})` : id === "default" ? "Focused Window" : id;
}
