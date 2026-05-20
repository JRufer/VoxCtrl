<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  const MODEL_SIZES = ["tiny","tiny.en","base","base.en","small","small.en",
    "medium","medium.en","large-v2","large-v3","large-v3-turbo"];
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
      <select bind:value={cfg.engine.whisper_cpp.model_size} onchange={markDirty}>
        {#each MODEL_SIZES as s}
          <option value={s}>{s}</option>
        {/each}
      </select>
    </label>
    <label class="field">
      <span>Device</span>
      <select bind:value={cfg.engine.whisper_cpp.device} onchange={markDirty}>
        <option value="auto">Auto</option>
        <option value="cuda">CUDA (NVIDIA)</option>
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
</style>
