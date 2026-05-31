<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

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
  });

  function markDirty() {
    configDirty.set(true);
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
      <span>Overlay style</span>
      <select bind:value={cfg.ui.overlay_style} onchange={markDirty}>
        <option value="voice_card">Voice Card</option>
        <option value="waveform">Waveform</option>
        <option value="pulse">Pulse Ring</option>
        <option value="blue_wave">Ocean Wave</option>
        {#each customOverlays as overlay}
          <option value={overlay.name}>{overlay.name}</option>
        {/each}
      </select>
    </label>

    <label class="field">
      <span>Overlay position</span>
      <select bind:value={cfg.ui.overlay_position} onchange={markDirty}>
        <option value="top">Top of screen</option>
        <option value="center">Center of screen</option>
        <option value="bottom">Bottom of screen</option>
      </select>
    </label>

    <label class="field">
      <span>Overlay display</span>
      <select bind:value={cfg.ui.overlay_monitor} onchange={markDirty}>
        <option value="primary">Primary Monitor</option>
        {#each monitors as mon}
          {#if mon.name}
            <option value={mon.name}>
              {mon.name} ({mon.width}x{mon.height}){mon.is_primary ? ' [Primary]' : ''}
            </option>
          {/if}
        {/each}
      </select>
    </label>

    {#if cfg.ui.overlay_monitor !== 'primary' && monitors.length > 0 && !monitors.some(m => m.name === cfg.ui.overlay_monitor)}
      <div class="warning-alert">
        <span>⚠️ Configured monitor "{cfg.ui.overlay_monitor}" is disconnected. Using Primary Monitor.</span>
      </div>
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
  @import "./tab.css";
</style>
