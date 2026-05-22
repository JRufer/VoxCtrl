<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  interface AudioDevice { index: number; name: string; }
  let devices = $state<AudioDevice[]>([]);

  // VU Meter States using Svelte 5 Runes
  let rawLevel = $state(0);
  let smoothedLevel = $state(0);
  let peakLevel = $state(0);
  let peakHoldTime = 0;
  let unlistenFn = $state<(() => void) | null>(null);

  let rafId: number;

  function updateVisuals() {
    const target = rawLevel;
    // Fast attack, slower decay physics
    if (target > smoothedLevel) {
      smoothedLevel += (target - smoothedLevel) * 0.45;
    } else {
      smoothedLevel += (target - smoothedLevel) * 0.12;
    }

    // Keep peak level with smooth decay
    if (smoothedLevel > peakLevel) {
      peakLevel = smoothedLevel;
      peakHoldTime = 25; // hold for ~0.4s
    } else {
      if (peakHoldTime > 0) {
        peakHoldTime--;
      } else {
        peakLevel = Math.max(0, peakLevel - 0.012);
      }
    }

    rafId = requestAnimationFrame(updateVisuals);
  }

  onMount(async () => {
    devices = await invoke<AudioDevice[]>("list_audio_devices");
    
    // Start backend microphone monitoring
    try {
      await invoke("start_monitoring_audio");
      unlistenFn = await listen<number>("audio-level", (event) => {
        rawLevel = event.payload;
      });
    } catch (e) {
      console.error("Failed to start audio monitoring:", e);
    }

    rafId = requestAnimationFrame(updateVisuals);
  });

  onDestroy(async () => {
    if (rafId) {
      cancelAnimationFrame(rafId);
    }
    if (unlistenFn) {
      unlistenFn();
    }
    // Stop backend microphone monitoring
    try {
      await invoke("stop_monitoring_audio");
    } catch (e) {
      console.error("Failed to stop audio monitoring:", e);
    }
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

    <!-- Hardware-inspired real-time VU meter -->
    <div class="vu-meter-container">
      <div class="vu-meter-info">
        <span class="vu-label">Microphone Monitor</span>
        <span class="vu-status-dot" class:active={rawLevel > 0.005}></span>
        <span class="vu-status-text">{rawLevel > 0.005 ? "Signal Detected" : "Listening..."}</span>
      </div>
      <div class="vu-meter-bar-wrapper">
        <div class="vu-meter-bar">
          {#each Array(24) as _, i}
            {@const threshold = i / 24}
            {@const isActive = smoothedLevel >= threshold}
            {@const isOrange = i >= 17}
            <div
              class="vu-segment"
              class:active={isActive}
              class:orange={isOrange}
            ></div>
          {/each}
          
          <!-- Peak marker line -->
          {#if peakLevel > 0.01}
            <div
              class="vu-peak-marker"
              style="left: {Math.min(0.99, peakLevel) * 100}%;"
            ></div>
          {/if}
        </div>
      </div>
    </div>

    <label class="field">
      <span>Dynamic Stream (Open on trigger)</span>
      <input type="checkbox" bind:checked={cfg.audio.dynamic_stream} onchange={markDirty} />
    </label>
    <p class="field-help" style="margin: -4px 0 12px 16px; opacity: 0.75; font-size: 11px; line-height: 1.45; color: var(--text-muted, #888); max-width: 480px;">
      Only opens the microphone stream when recording is triggered. If your first words are being clipped due to hardware delay, disable this feature to keep the stream always open.
    </p>
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

<style>
  @import "./tab.css";
  .val { font-size: 12px; color: var(--text-muted); min-width: 36px; text-align: right; }

  .vu-meter-container {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 4px 0;
    margin: 4px 0 12px 0;
  }

  .vu-meter-info {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10px;
    font-family: monospace;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }

  .vu-label {
    color: var(--text-muted);
    font-weight: 600;
  }

  .vu-status-dot {
    width: 6px;
    height: 6px;
    background: #444;
    transition: background 0.15s ease;
  }

  .vu-status-dot.active {
    background: #39ff14; /* Neon Acid Green */
    box-shadow: 0 0 8px #39ff14;
  }

  .vu-status-text {
    color: var(--text);
    opacity: 0.7;
    margin-left: auto;
  }

  .vu-meter-bar-wrapper {
    position: relative;
    width: 100%;
    height: 14px;
    background: rgba(0, 0, 0, 0.4);
    padding: 3px;
    box-sizing: border-box;
  }

  .vu-meter-bar {
    position: relative;
    display: flex;
    gap: 2px;
    width: 100%;
    height: 100%;
  }

  .vu-segment {
    flex: 1;
    height: 100%;
    background: rgba(255, 255, 255, 0.05);
    transition: background 0.05s ease;
  }

  /* Neon Acid Green for safe range */
  .vu-segment.active {
    background: #39ff14;
    box-shadow: 0 0 4px rgba(57, 255, 20, 0.3);
  }

  /* Signal Orange for warning range */
  .vu-segment.active.orange {
    background: #ff6d00;
    box-shadow: 0 0 4px rgba(255, 109, 0, 0.5);
  }

  .vu-peak-marker {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 2px;
    background: #ffab00;
    box-shadow: 0 0 6px #ffab00;
    pointer-events: none;
    transition: left 0.05s linear;
  }
</style>
