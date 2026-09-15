# Custom overlay styles

Every subfolder in this directory that contains an `index.html` and a
`style.css` becomes a selectable **Overlay style** in VoxCtrl's
**Settings → Visual & Feedback** tab, named after the folder. New or
renamed folders show up the next time that dropdown is opened. Both files
are read fresh, together, every time you select that style in the
dropdown — including re-selecting the style you're already on — and
again every time the overlay next activates (a dictation starts). No app
restart needed either way, and there's no per-file caching to go stale:
edit `index.html`, `style.css`, or both, and the very next selection or
activation shows the current files.

## Getting started

The `Custom/` folder next to this file is a complete, working example — a
copy of the built-in Voice Card style with one line changed (`VOXCTRL` →
`CUSTOM OVERLAY`), so you can compare the two and see exactly what a real
overlay folder looks like. Open `Custom/index.html` for a full walkthrough
of the templating placeholder and the live CSS custom properties VoxCtrl
writes while your overlay is on screen.

**VoxCtrl makes sure `Custom/` exists, and never touches it again once it
does** — edited or not. Feel free to experiment directly in `Custom/`
first; nothing you change there will be reset. Delete the whole `Custom/`
folder if you ever want the default example back. To keep your own style
alongside it instead of replacing it:

1. Duplicate `Custom/` and rename the copy — the new folder's name becomes
   the style's name in Settings.
2. Edit its `index.html` / `style.css`.
3. Pick it from **Settings → Visual & Feedback → Overlay style**.

## Folder format

```
overlays/
├── Custom/            (this example)
│   ├── index.html
│   └── style.css
└── YourStyleName/
    ├── index.html
    └── style.css
```

- Both files are read as plain text and injected into the overlay window.
  **`<script>` tags do not run** — the app's content-security-policy
  (`script-src 'self'`) blocks inline script everywhere, including here,
  with no visible error; drive your overlay from CSS instead (see below).
- The folder name is shown as-is in the Overlay style dropdown. If it
  matches a built-in style's internal name (`voice_card`, `waveform`,
  `pulse`, `blue_wave`, `mono_bars`, `spectrum`, `terminal`, `vinyl`, or
  `none`), VoxCtrl appends `_custom` to keep it selectable without
  clashing with the built-in.
- Deleting a folder you made removes it from the dropdown, permanently.
  This README is rewritten on every launch unconditionally; `Custom/` is
  the exception — VoxCtrl only ever creates it when it's missing, so
  deleting it is also how you ask for the default example back.

## Quick reference: placeholder and live state

See `Custom/index.html` and `style.css` for the full explanation and a
working example of all of this — this is the short version:

**Placeholder** (substituted once, into the initial HTML):
- `{{target}}` / `{{trigger}}` — the active routing target's label.

**Live state** (CSS custom properties VoxCtrl continuously writes onto the
page root while the overlay is visible — read them with `var()`/`calc()`,
no script needed):
- `--voxctrl-recording`, `--voxctrl-processing`, `--voxctrl-speaking`,
  `--voxctrl-mcp-recording`, `--voxctrl-audio-ready` — each `0` or `1`.
- `--voxctrl-audio-level` — `0`..`1`, already smoothed.

CSS can't swap text content based on a variable, so for anything that
needs to (a status label that changes text, not just color) the trick
`Custom/` uses is to pre-render every possible label stacked on top of
each other and toggle each one's `opacity` with the formula that should
show it — see `.vc-custom-stamp-text` in `Custom/style.css`.
