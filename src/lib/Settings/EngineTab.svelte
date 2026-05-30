<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  const MODEL_SIZES = ["tiny","tiny.en","base","base.en","small","small.en",
    "medium","medium.en","large-v2","large-v3","large-v3-turbo"];

  let downloadedMap = $state<Record<string, boolean>>({});
  let checking = $state(false);
  let downloading = $state(false);
  let cudaEnabled = $state(false);

  async function checkAllModelsDownloaded() {
    checking = true;
    const newMap: Record<string, boolean> = {};
    for (const m of MODEL_SIZES) {
      try {
        newMap[m] = await invoke<boolean>("check_model_downloaded", { modelSize: m });
      } catch (e) {
        console.error("Failed to check download status for model " + m, e);
        newMap[m] = false;
      }
    }
    downloadedMap = newMap;
    checking = false;
  }

  async function triggerDownload(model: string) {
    if (downloading) return;
    downloading = true;
    try {
      await invoke("download_model", { modelSize: model });
      downloadedMap[model] = true;
    } catch (e) {
      alert(`Failed to download model: ${e}`);
    } finally {
      downloading = false;
    }
  }

  async function onModelChanged() {
    markDirty();
    const selected = cfg.engine.whisper_cpp.model_size;
    if (!downloadedMap[selected]) {
      await triggerDownload(selected);
    }
  }

  onMount(async () => {
    checkAllModelsDownloaded();
    cudaEnabled = await invoke<boolean>("cuda_enabled");
    // If this is a CPU build but config still says "cuda", reset to "auto"
    if (!cudaEnabled && cfg.engine.whisper_cpp.device === "cuda") {
      cfg.engine.whisper_cpp.device = "auto";
      markDirty();
    }
  });
</script>

<section>
  <h2>Inference Engine</h2>

  <div class="field-group">
    <h3>Backend</h3>
    <label class="field">
      <span>Backend</span>
      <select bind:value={cfg.engine.backend} onchange={markDirty}>
        <option value="auto">Auto-detect</option>
        <option value="whisper-cpp">Whisper.cpp</option>
        <option value="moonshine">Moonshine (CPU only)</option>
      </select>
    </label>
    <label class="field">
      <span>Inference mode</span>
      <select bind:value={cfg.engine.inference_mode} onchange={markDirty}>
        <option value="Balanced">Balanced</option>
        <option value="Aggressive">Aggressive (shorter silence)</option>
      </select>
    </label>
  </div>

  {#if cfg.engine.backend !== "moonshine"}
  <div class="field-group">
    <h3>Whisper.cpp Settings</h3>
    <label class="field">
      <span>Model size</span>
      <select bind:value={cfg.engine.whisper_cpp.model_size} onchange={onModelChanged}>
        {#each MODEL_SIZES as s}
          <option value={s}>{s}{downloadedMap[s] ? " ✔" : ""}</option>
        {/each}
      </select>
    </label>

    <div class="model-status-container">
      {#if checking}
        <span class="status-checking">⏳ Checking local model files...</span>
      {:else if downloading}
        <span class="status-downloading">⏳ Downloading {cfg.engine.whisper_cpp.model_size} (GGUF format)...</span>
      {:else if downloadedMap[cfg.engine.whisper_cpp.model_size]}
        <span class="status-downloaded">✔ Model downloaded and ready</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class="status-missing">❌ Model file missing</span>
          <button class="btn-download" onclick={() => triggerDownload(cfg.engine.whisper_cpp.model_size)}>
            📥 Download Model
          </button>
        </div>
      {/if}
    </div>

    <label class="field">
      <span>Device</span>
      <select bind:value={cfg.engine.whisper_cpp.device} onchange={markDirty}>
        <option value="auto">Auto</option>
        {#if cudaEnabled}
        <option value="cuda">CUDA (NVIDIA)</option>
        {/if}
        <option value="vulkan">Vulkan (AMD/Intel)</option>
        <option value="cpu">CPU</option>
      </select>
    </label>
    <label class="field">
      <span>Model directory (leave blank for default)</span>
      <input type="text" bind:value={cfg.engine.whisper_cpp.model_dir} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Threads (0 = auto)</span>
      <input type="number" min="0" max="64" bind:value={cfg.engine.whisper_cpp.threads} onchange={markDirty} />
    </label>
    <p class="hint">Default model directory: <code>~/.local/share/voxctl/models/</code></p>
  </div>
  {:else}
  <div class="field-group">
    <h3>Moonshine Settings</h3>
    <label class="field">
      <span>Model size</span>
      <select bind:value={cfg.engine.moonshine.model_size} onchange={markDirty}>
        <option value="base">Base</option>
        <option value="tiny">Tiny</option>
      </select>
    </label>
    <label class="field">
      <span>Language</span>
      <input type="text" bind:value={cfg.engine.moonshine.language} onchange={markDirty} />
    </label>
  </div>
  {/if}
</section>

<style>
  @import "./tab.css";

  .model-status-container {
    display: flex;
    align-items: center;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
    min-height: 42px;
    margin-bottom: 12px;
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
