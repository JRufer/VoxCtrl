# Pasting Dictation

By default VoxCtrl delivers dictated text to the focused window as **one paste**
rather than one key press per character. This is what the `inject` target does
(see [Routing](./routing.md#inject--keystroke-injection)); this page explains how
it works, what each platform needs, and how to diagnose it.

Why paste: typing sends hundreds of key events, so if focus is somewhere that is
not a text field — a game, a file manager, a terminal running a program that
reads single keys — every character is a stray key press. A paste is a single
shortcut. Typing remains the fallback.

---

## What happens on every dictation

1. **Choose the shortcut** for the focused window (below).
2. **Back up the whole clipboard** — every format it offers, not just text.
3. **Put the dictation on the clipboard**, asking clipboard managers and cloud
   history not to record it.
4. **Send the paste shortcut.**
5. **Wait** for the application to read it (at least 300 ms; on X11 VoxCtrl can
   see the read and waits for slow applications, up to 2 s).
6. **Restore the clipboard** — but only if it still holds the dictation. If you
   copied something else in the meantime, that is yours and is left alone.

If any step cannot be done, the text is **typed instead**, so a dictation is
never silently dropped. Every step has a time limit, so one stalled clipboard
owner cannot hang dictation. Dictations are delivered one at a time, so two in
quick succession cannot share the clipboard.

Text longer than 2,000 characters is pasted on Windows even when the setting is
off (typing it makes the target process one message per character).

## Settings

**Settings → Features → Text Delivery**, or `config.json` (see
[Configuration](./configuration.md#features-section)):

| Key | Default | |
|---|---|---|
| `features.paste_instead_of_typing` | `true` | Paste instead of typing. Off = always type. |
| `features.paste_shortcut` | `"auto"` | `auto`, `ctrl+v`, `ctrl+shift+v` or `shift+insert`. Force one only for a window VoxCtrl cannot identify. |

### Disabled where it does not work

**Linux Mint** is detected (`/etc/os-release`, `ID=linuxmint`, which also covers
LMDE) and pasting is switched off there: the setting is greyed out with the
reason, and text is always typed. Set `VOXCTRL_FORCE_PASTE=1` to override this
when testing.

## Choosing the paste shortcut

`auto` uses Ctrl+V, except where a window needs something else:

| Window | Shortcut | How it is recognised |
|---|---|---|
| Linux terminal emulators (GNOME Terminal, Konsole, Alacritty, kitty, foot, WezTerm, xterm, …) | Ctrl+Shift+V | X11: `WM_CLASS` of the active window. Wayland: Hyprland (`hyprctl`), sway (`swaymsg`) or KDE (`kdotool`) if installed |
| Windows console, mintty, PuTTY, ConEmu | Shift+Insert | Window class |
| Everything else | Ctrl+V | |

On a Wayland desktop that cannot say which window is focused (GNOME), terminals
cannot be recognised: set `paste_shortcut` to `ctrl+shift+v` if that is what you
paste into.

## The clipboard backup

Everything is held in memory for the length of one paste and is never written to
disk or logged.

| Platform | What is saved |
|---|---|
| **Windows** | Every format backed by a memory block: text, HTML, RTF, images (`CF_DIB`/`CF_DIBV5`), file lists (`CF_HDROP`), registered and private formats. Formats that are GDI handles (`CF_BITMAP`, metafiles, palettes) are skipped; Windows rebuilds them from the memory formats. |
| **X11** | Every target the owner offers, including large ones sent in chunks (`INCR`). X11 has no clipboard store, so after a restore VoxCtrl itself serves the saved formats until something else takes the clipboard — if VoxCtrl exits and no clipboard manager holds them, they are gone. |
| **Wayland** | Every MIME type, through the data-control protocol (wlroots compositors, KDE). GNOME has no such protocol, so it goes through XWayland like X11. |

An item larger than 256 MB, or a format an application never finishes sending,
is skipped (the others are kept). If *nothing* could be read, the clipboard is
treated as unreadable, **not** as empty — an empty-clipboard restore would clear it.

## Sending the shortcut

| Session | Tried in order |
|---|---|
| **Windows** | `SendInput` |
| **X11** | XTEST key events (no helper needed), then `xdotool` |
| **Wayland** | `wtype`, `ydotool`, the **RemoteDesktop portal**, then X11 (reaches XWayland windows only — VoxCtrl logs a warning) |

Held modifier keys (typically the dictation hotkey) are released first, so they
cannot turn Ctrl+V into a different shortcut.

### The RemoteDesktop portal (Wayland)

GNOME and KDE offer no virtual-keyboard protocol for `wtype`, and X11 key events
reach only XWayland windows. The sanctioned route is the
`org.freedesktop.portal.RemoteDesktop` portal:

- The **first dictation** shows your desktop's permission dialog ("allow VoxCtrl
  to control the keyboard"). It appears before the clipboard is touched.
- The grant is remembered in `~/.config/voxctrl/portal-keyboard-token`, so later
  launches do not ask again. Revoke it in your desktop's settings, or delete that file.
- If you decline, or the portal is missing, VoxCtrl remembers that until it is
  restarted and falls back; the log says so. Installing `ydotool` is the
  alternative.

See [Privacy](./privacy.md#sending-keys-the-paste-shortcut) for what this permits.

## Troubleshooting

VoxCtrl logs each stage. The default log filter includes the paste crates;
for more detail:

```bash
RUST_LOG=voxctrl_inject=debug,voxctrl_clipboard=debug,voxctrl_routing=info voxctrl
```

| Log line | Meaning |
|---|---|
| `clipboard backed up backend=… formats=N` | Backup worked. `formats=0` means the clipboard was genuinely empty |
| `clipboard format … could not be backed up` | One format never finished sending and was skipped; it will not be restored |
| `could not back up the clipboard … it will not be restored` | The backup failed entirely; the dictation is still pasted, but your previous clipboard is lost |
| `paste shortcut sent shortcut=… ok=…` | Which shortcut was sent, and whether the sender reported success |
| `nothing could send the paste shortcut to native Wayland windows` | No `wtype`/`ydotool`/portal; only XWayland windows will receive it |
| `RemoteDesktop portal unavailable` | Permission declined, or no portal on this desktop |
| `clipboard restored formats=N` | Restored |
| `the clipboard changed during the paste … left alone` | You copied something meanwhile; nothing was overwritten |
| `paste failed (…); typing instead` | The paste could not be done; the text was typed |

Common problems:

- **Nothing pastes into a terminal** — set `paste_shortcut` to `ctrl+shift+v`.
- **Nothing pastes into a native Wayland window** — install `ydotool`, or accept
  the portal permission dialog.
- **Your clipboard comes back after a delay, not immediately** — expected: the
  restore waits for the application to read the dictation.

## Manual test matrix

Automated tests cover the X11 path end to end (a stand-in application receives
the shortcut and reads the clipboard; multi-format backup/restore; a stalled
owner; a clipboard manager taking over; concurrent dictations), the Wayland
backend on a headless sway, and the RemoteDesktop portal flow against a
stand-in portal. Windows and the real desktop portals are checked by hand:

| Check | Expect |
|---|---|
| Copy text, dictate into a text editor, press Ctrl+V | The **copied** text, not the dictation |
| Copy an image (screenshot), dictate, paste the image into an image editor | The image is intact |
| Copy files in a file manager, dictate, paste into the file manager | The files |
| Dictate into a terminal | Pasted, no `^V` |
| Dictate with a game or file manager focused | Nothing is typed |
| Dictate twice quickly | Both appear, in order |
| Dictate with a clipboard manager running | The dictation is not in its history; your copy is restored |
| (Wayland) First dictation | The permission dialog appears once, before anything is pasted |
| (Linux Mint) Settings → Features | The paste toggle is greyed out with the reason |
