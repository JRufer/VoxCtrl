<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  const PIPER_VOICES = [
    "en-us-lessac-medium", "en-us-lessac-high", "en-us-ryan-medium", "en-us-ryan-high",
    "en-gb-alan-medium", "en-gb-jenny-dioco-medium", "de-thorsten-medium",
    "fr-upmc-medium", "es-carlfm-x-low", "it-riccardo-x-low",
  ];

  async function previewVoice() {
    await invoke("speak_text", { text: "Hello, this is a voice preview from VoxCtr." });
  }
</script>

<section>
  <h2>Text to Speech</h2>

  <div class="field-group">
    <h3>TTS Engine</h3>
    <label class="field">
      <span>Enable TTS</span>
      <input type="checkbox" bind:checked={cfg.tts.enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Engine</span>
      <select bind:value={cfg.tts.engine} onchange={markDirty}>
        <option value="piper">Piper (neural, high quality)</option>
        <option value="espeak">eSpeak-NG (lightweight)</option>
      </select>
    </label>
  </div>

  {#if cfg.tts.engine === "piper"}
  <div class="field-group">
    <h3>Piper Voice</h3>
    <label class="field">
      <span>Voice</span>
      <select bind:value={cfg.tts.voice} onchange={markDirty}>
        {#each PIPER_VOICES as v}
          <option value={v}>{v}</option>
        {/each}
      </select>
    </label>
    <div class="row">
      <button class="btn-preview" onclick={previewVoice} disabled={!cfg.tts.enabled}>
        Preview Voice
      </button>
    </div>
  </div>
  {/if}

  <div class="field-group">
    <h3>Playback</h3>
    <label class="field">
      <span>Show response overlay</span>
      <input type="checkbox" bind:checked={cfg.tts.response_overlay} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Stop key(s)</span>
      <input type="text"
        value={cfg.tts.stop_key.join(", ")}
        onchange={(e) => {
          cfg.tts.stop_key = (e.target as HTMLInputElement).value.split(",").map(s => s.trim());
          markDirty();
        }}
      />
    </label>
  </div>
</section>

<style>
  @import "./tab.css";
  .row { display: flex; gap: 8px; }
  .btn-preview {
    background: var(--surface2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 6px 14px;
    font-size: 12px;
  }
  .btn-preview:disabled { opacity: 0.4; }
</style>
