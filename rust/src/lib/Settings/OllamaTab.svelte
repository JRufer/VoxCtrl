<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }
</script>

<section>
  <h2>Ollama LLM Post-Processing</h2>

  <div class="field-group">
    <h3>Connection</h3>
    <label class="field">
      <span>Enable Ollama</span>
      <input type="checkbox" bind:checked={cfg.ollama.enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Endpoint</span>
      <input type="text" bind:value={cfg.ollama.endpoint} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Model</span>
      <input type="text" bind:value={cfg.ollama.model} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Timeout (seconds)</span>
      <input type="number" min="1" max="60" bind:value={cfg.ollama.timeout_secs} onchange={markDirty} />
    </label>
  </div>

  <div class="field-group">
    <h3>Processing Mode</h3>
    <label class="field">
      <span>Mode</span>
      <select bind:value={cfg.ollama.mode} onchange={markDirty}>
        <option value="clean">Clean (grammar fix)</option>
        <option value="formal">Formal</option>
        <option value="casual">Casual</option>
        <option value="bullet">Bullet points</option>
        <option value="concise">Concise (summarize)</option>
        <option value="custom">Custom prompt</option>
      </select>
    </label>
    {#if cfg.ollama.mode === "custom"}
    <label class="field col">
      <span>Custom prompt template (<code>{"{text}"}</code> is substituted)</span>
      <textarea
        rows="4"
        bind:value={cfg.ollama.custom_prompt}
        onchange={markDirty}
      ></textarea>
    </label>
    {/if}
  </div>
</section>

<style src="./tab.css"></style>
<style>
  .field.col { flex-direction: column; align-items: flex-start; gap: 6px; }
  textarea {
    width: 100%;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    padding: 8px;
    font-size: 13px;
    font-family: monospace;
    resize: vertical;
  }
</style>
