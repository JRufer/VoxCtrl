<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  interface AudioDevice { index: number; name: string; }
  let devices = $state<AudioDevice[]>([]);

  onMount(async () => {
    devices = await invoke<AudioDevice[]>("list_audio_devices");
  });
</script>

<section>
  <h2>Audio</h2>

  <div class="field-group">
    <h3>Input Device</h3>
    <label class="field">
      <span>Microphone</span>
      <select
        value={cfg.audio.input_device_index ?? -1}
        onchange={(e) => {
          const v = parseInt((e.target as HTMLSelectElement).value);
          cfg.audio.input_device_index = v < 0 ? null : v;
          markDirty();
        }}
      >
        <option value={-1}>Default system device</option>
        {#each devices as d}
          <option value={d.index}>{d.name}</option>
        {/each}
      </select>
    </label>
  </div>

  <div class="field-group">
    <h3>Voice Activity Detection</h3>
    <label class="field">
      <span>VAD threshold (0.0 – 1.0)</span>
      <input
        type="range"
        min="0"
        max="1"
        step="0.05"
        bind:value={cfg.audio.vad_threshold}
        onchange={markDirty}
      />
      <span class="val">{cfg.audio.vad_threshold.toFixed(2)}</span>
    </label>
    <label class="field">
      <span>Silence duration (ms)</span>
      <input type="number" min="100" max="5000" step="50"
        bind:value={cfg.audio.min_silence_duration_ms} onchange={markDirty} />
    </label>
  </div>

  <div class="field-group">
    <h3>Processing</h3>
    <label class="field">
      <span>Noise suppression (RNNoise)</span>
      <input type="checkbox" bind:checked={cfg.audio.noise_suppression} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Input gain</span>
      <input type="range" min="0.5" max="4.0" step="0.1"
        bind:value={cfg.audio.gain} onchange={markDirty} />
      <span class="val">{cfg.audio.gain.toFixed(1)}×</span>
    </label>
  </div>
</section>

<style src="./tab.css"></style>
<style>
  .val { font-size: 12px; color: var(--text-muted); min-width: 36px; text-align: right; }
</style>
