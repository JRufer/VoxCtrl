<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() { configDirty.set(true); }

  const PIPER_VOICES = [
    "en-us-libritts-high",
    "en-us-amy-low",
    "en-us-kathleen-low",
    "en-gb-southern_english_female-low",
    "en-us-ryan-high",
    "en-us-ryan-medium",
    "en-us-ryan-low",
    "en-us-lessac-medium",
    "en-us-lessac-low",
    "en-us-danny-low",
    "en-gb-alan-low"
  ];

  let downloadedMap = $state<Record<string, boolean>>({});
  let checking = $state(false);
  let downloading = $state(false);

  async function checkAllVoicesDownloaded() {
    checking = true;
    const newMap: Record<string, boolean> = {};
    for (const v of PIPER_VOICES) {
      try {
        newMap[v] = await invoke<boolean>("check_voice_downloaded", { voiceName: v });
      } catch (e) {
        console.error("Failed to check download status for voice " + v, e);
        newMap[v] = false;
      }
    }
    downloadedMap = newMap;
    checking = false;
  }

  async function triggerDownload(voice: string) {
    if (downloading) return;
    downloading = true;
    try {
      await invoke("download_voice", { voiceName: voice });
      downloadedMap[voice] = true;
    } catch (e) {
      alert(`Failed to download voice: ${e}`);
    } finally {
      downloading = false;
    }
  }

  async function onVoiceChanged() {
    markDirty();
    const selected = cfg.tts.voice;
    if (!downloadedMap[selected]) {
      await triggerDownload(selected);
    }
  }

  async function previewVoice() {
    await invoke("speak_text", { 
      text: "Hello, this is a voice preview from VoxCtr.",
      voice: cfg.tts.voice 
    });
  }

  onMount(() => {
    checkAllVoicesDownloaded();
  });
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
      <select bind:value={cfg.tts.voice} onchange={onVoiceChanged}>
        {#each PIPER_VOICES as v}
          <option value={v}>
            {v}{downloadedMap[v] ? " ✔" : ""}
          </option>
        {/each}
      </select>
    </label>

    <div class="voice-status-container">
      {#if checking}
        <span class="status-checking">⏳ Checking local voice files...</span>
      {:else if downloading}
        <span class="status-downloading">⏳ Downloading {cfg.tts.voice} (model + config)...</span>
      {:else if downloadedMap[cfg.tts.voice]}
        <span class="status-downloaded">✔ Voice downloaded and ready</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class="status-missing">❌ Voice files missing</span>
          <button class="btn-download" onclick={() => triggerDownload(cfg.tts.voice)}>
            📥 Download Voice
          </button>
        </div>
      {/if}
    </div>

    <div class="row">
      <button class="btn-preview" onclick={previewVoice} disabled={!cfg.tts.enabled || downloading || !downloadedMap[cfg.tts.voice]}>
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
  .row { display: flex; gap: 8px; margin-top: 8px; }
  .btn-preview {
    background: var(--surface2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 6px 14px;
    font-size: 12px;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .btn-preview:hover:not(:disabled) {
    background: var(--border);
    color: var(--accent);
  }
  .btn-preview:disabled { opacity: 0.4; cursor: not-allowed; }

  .voice-status-container {
    display: flex;
    align-items: center;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
    min-height: 42px;
  }
  .status-downloaded {
    color: #4caf50;
    font-weight: 600;
  }
  .status-downloading {
    color: var(--accent2);
  }
  .status-checking {
    color: var(--text-muted);
  }
  .status-missing-wrapper {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
  }
  .status-missing {
    color: #e57373;
  }
  .btn-download {
    background: var(--accent);
    border: none;
    color: #fff;
    border-radius: var(--radius);
    padding: 6px 12px;
    font-size: 12px;
    cursor: pointer;
    font-weight: 600;
    transition: background 0.2s;
  }
  .btn-download:hover {
    background: var(--accent2);
  }
</style>
