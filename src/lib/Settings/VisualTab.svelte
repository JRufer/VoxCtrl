<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();

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
        <option value="none">Disabled</option>
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
