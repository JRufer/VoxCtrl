<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();

  interface CustomOverlay {
    name: string;
    html: string;
    css: string;
  }

  let customOverlays = $state<CustomOverlay[]>([]);

  onMount(async () => {
    try {
      customOverlays = await invoke<CustomOverlay[]>("get_custom_overlays");
    } catch (e) {
      console.error("Failed to fetch custom overlays:", e);
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
