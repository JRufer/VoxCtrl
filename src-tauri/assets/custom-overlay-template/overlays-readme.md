# Custom overlay styles

Every subfolder in this directory that contains an `index.html` and a
`style.css` becomes a selectable **Overlay style** in VoxCtrl's
**Settings → Visual & Feedback** tab, named after the folder. VoxCtrl
rescans this directory each time the Overlay style dropdown is opened, so
new or edited folders show up without restarting the app.

## Getting started

The `Custom/` folder next to this file is a complete, working example — a
copy of the built-in Voice Card style with one line changed (`VOXCTRL` →
`USER CUSTOM`), so you can compare the two and see exactly what a real
overlay folder looks like. Open `Custom/index.html` for a full walkthrough
of the templating placeholders and the live events VoxCtrl dispatches
while your overlay is on screen.

The fastest way to start your own style:

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

- Both files are read as plain text and injected into the overlay window;
  `index.html`'s `<style>`/`<script>` tags work normally.
- The folder name is shown as-is in the Overlay style dropdown. If it
  matches a built-in style's internal name (`voice_card`, `waveform`,
  `pulse`, `blue_wave`, `mono_bars`, `spectrum`, `terminal`, `vinyl`, or
  `none`), VoxCtrl appends `_custom` to keep it selectable without
  clashing with the built-in.
- Deleting a folder removes it from the dropdown; VoxCtrl does not
  recreate folders you've deleted, including `Custom/`.

## Quick reference: placeholders and events

See `Custom/index.html` for the full explanation — this is the short
version:

**Placeholders** (substituted once, into the initial HTML):
- `{{target}}` / `{{trigger}}` — the active routing target's label.

**Live events** (dispatched on `window` while the overlay is visible):
- `voxctrl-status` — fires ~60 times/second with
  `{ recording, processing, speaking, audio_ready, active_target_label,
  audio_level }` in `event.detail`. `audio_level` is 0..1 and already
  smoothed.
- `voxctrl-audio-level` — raw, unsmoothed microphone level (0..1) on every
  buffer, if you want to do your own smoothing.
- `voxctrl-cleanup` — fires once when your overlay is switched away from;
  remove your own event listeners here.
