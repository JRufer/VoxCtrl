<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { emit } from "@tauri-apps/api/event";
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();

  interface CustomOverlay {
    name: string;
    html: string;
    css: string;
  }

  interface MonitorInfo {
    name: string | null;
    width: number;
    height: number;
    is_primary: boolean;
  }

  let customOverlays = $state<CustomOverlay[]>([]);
  let monitors = $state<MonitorInfo[]>([]);
  let customOverlaysDir = $state("");

  let overlayStyleOptions = $derived([
    { value: "voice_card", label: "Voice Card" },
    { value: "waveform", label: "Waveform" },
    { value: "pulse", label: "Pulse Ring" },
    { value: "blue_wave", label: "Ocean Wave" },
    { value: "mono_bars", label: "Mono Bars" },
    { value: "spectrum", label: "Neon Spectrum" },
    { value: "terminal", label: "Retro Terminal" },
    { value: "vinyl", label: "Analog VU" },
    ...customOverlays.map(o => ({ value: o.name, label: o.name }))
  ]);

  const overlayPositionOptions = [
    { value: "top", label: "Top of screen" },
    { value: "center", label: "Center of screen" },
    { value: "bottom", label: "Bottom of screen" }
  ];

  let overlayMonitorOptions = $derived([
    { value: "primary", label: "Primary Monitor" },
    ...monitors.filter(mon => mon.name).map(mon => ({
      value: mon.name!,
      label: `${mon.name} (${mon.width}x${mon.height})${mon.is_primary ? ' [Primary]' : ''}`
    }))
  ]);

  onMount(async () => {
    try {
      customOverlays = await invoke<CustomOverlay[]>("get_custom_overlays");
    } catch (e) {
      console.error("Failed to fetch custom overlays:", e);
    }
    try {
      monitors = await invoke<MonitorInfo[]>("get_available_monitors");
    } catch (e) {
      console.error("Failed to fetch available monitors:", e);
    }
    try {
      customOverlaysDir = await invoke<string>("get_custom_overlays_dir");
    } catch (e) {
      console.error("Failed to fetch custom overlays directory:", e);
    }
  });

  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  // The overlay window's own config store only reacts when the value it
  // receives actually differs from what it already has — Settings
  // auto-saves on a debounce, so picking a style, then a different one,
  // then back to the first within that window can collapse into a single
  // save equal to the original value, which the overlay window never sees
  // as a change. That's fine for most fields, but for a custom overlay it
  // means re-selecting a style you just edited can silently fail to
  // re-read its (possibly changed) index.html/style.css. This event
  // sidesteps the config store entirely: every selection, including
  // re-selecting the same value, tells the overlay window directly to
  // re-read that style's files fresh from disk right now.
  function onOverlayStyleChange(val: string) {
    markDirty();
    emit("overlay-style-selected", val);
  }
</script>

<section>
  <h2>Visual & Feedback</h2>

  <div class="field-group">
    <h3>Overlay & HUD</h3>
    <label class="field">
      <span>Show overlay while speaking</span>
      <input type="checkbox" bind:checked={cfg.ui.show_overlay} onchange={markDirty} />
    </label>

    <label class="field">
      <span>Show overlay on voice command trigger</span>
      <input type="checkbox" bind:checked={cfg.ui.show_command_overlay} onchange={markDirty} />
    </label>

    {#if cfg.ui.show_command_overlay}
      <label class="field">
        <span>Command overlay duration (seconds)</span>
        <input
          type="number"
          min="1"
          max="10"
          step="1"
          bind:value={cfg.ui.command_overlay_duration_secs}
          onchange={markDirty}
        />
      </label>
    {/if}
    
    <label class="field">
      <span>Overlay style</span>
      <CustomSelect bind:value={cfg.ui.overlay_style} options={overlayStyleOptions} onchange={onOverlayStyleChange} />
    </label>

    <label class="field">
      <span>Overlay position</span>
      <CustomSelect bind:value={cfg.ui.overlay_position} options={overlayPositionOptions} onchange={markDirty} />
    </label>

    <label class="field">
      <span>Overlay display</span>
      <CustomSelect bind:value={cfg.ui.overlay_monitor} options={overlayMonitorOptions} onchange={markDirty} />
    </label>

    {#if cfg.ui.overlay_monitor !== 'primary' && monitors.length > 0 && !monitors.some(m => m.name === cfg.ui.overlay_monitor)}
      <div class="warning-alert">
        <span>⚠️ Configured monitor "{cfg.ui.overlay_monitor}" is disconnected. Using Primary Monitor.</span>
      </div>
    {/if}

    {#if customOverlaysDir}
      <p class="hint">
        Design your own overlay style by adding a folder with an
        <code>index.html</code> and <code>style.css</code> to the folder below — it
        shows up here as a selectable style automatically. A working example
        (a copy of Voice Card, ready to duplicate) and a README covering the
        format live there already.
      </p>
      <p class="hint">Custom overlay styles: <code>{customOverlaysDir}</code></p>
    {/if}
  </div>

  <div class="field-group">
    <h3>Alerts & OS Integration</h3>
    <label class="field">
      <span>Show system notifications on transcription</span>
      <input type="checkbox" bind:checked={cfg.ui.show_notification} onchange={markDirty} />
    </label>
  </div>

  <div class="field-group">
    <h3>Application Window</h3>
    <label class="field">
      <span>Automatically open settings at launch</span>
      <input type="checkbox" bind:checked={cfg.ui.auto_show_settings} onchange={markDirty} />
    </label>
  </div>
</section>

<style>
</style>
