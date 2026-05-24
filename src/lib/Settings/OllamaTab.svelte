<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  interface TestResult {
    success: boolean;
    message: string;
    models: string[];
  }

  let testing = $state(false);
  let testStatus = $state<{ success: boolean; message: string } | null>(null);
  let availableModels = $state<string[]>([]);

  async function performTest() {
    testing = true;
    testStatus = null;
    try {
      const res = await invoke<TestResult>("test_ollama", {
        endpoint: cfg.ollama.endpoint,
        timeoutSecs: cfg.ollama.timeout_secs,
      });
      testStatus = { success: res.success, message: res.message };
      if (res.success) {
        availableModels = res.models;
        // If our current model is empty but models are returned, auto-select the first one
        if (!cfg.ollama.model && res.models.length > 0) {
          cfg.ollama.model = res.models[0];
          markDirty();
        }
      }
    } catch (e: any) {
      testStatus = { success: false, message: e.toString() };
    } finally {
      testing = false;
    }
  }

  onMount(() => {
    // Try to silently probe/load models on mount
    performTest();
  });
</script>

<section>
  <h2>Ollama LLM Post-Processing</h2>

  <div class="field-group">
    <h3>Connection</h3>
    <label class="field">
      <span>Endpoint</span>
      <input type="text" bind:value={cfg.ollama.endpoint} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Model (Default)</span>
      {#if availableModels.length > 0}
        <select bind:value={cfg.ollama.model} onchange={markDirty}>
          {#each availableModels as model}
            <option value={model}>{model}</option>
          {/each}
          {#if cfg.ollama.model && !availableModels.includes(cfg.ollama.model)}
            <option value={cfg.ollama.model}>{cfg.ollama.model} (not found)</option>
          {/if}
        </select>
      {:else}
        <input type="text" bind:value={cfg.ollama.model} onchange={markDirty} placeholder="e.g. llama3.2:1b" />
      {/if}
    </label>
    <label class="field">
      <span>Timeout (seconds)</span>
      <input type="number" min="1" max="60" bind:value={cfg.ollama.timeout_secs} onchange={markDirty} />
    </label>

    <div class="action-row">
      <button class="btn-test" onclick={performTest} disabled={testing}>
        {testing ? "⏳ Testing..." : "🔌 Test Connection"}
      </button>
      {#if testStatus}
        <span class="status-msg {testStatus.success ? 'success' : 'error'}">
          {testStatus.message}
        </span>
      {/if}
    </div>
  </div>

  <div class="field-group">
    <h3>Default Processing Mode</h3>
    <label class="field">
      <span>Mode</span>
      <select bind:value={cfg.ollama.mode} onchange={markDirty}>
        <option value="clean">Clean (grammar fix)</option>
        <option value="formal">Formal</option>
        <option value="casual">Casual</option>
        <option value="bullet">Bullet points</option>
        <option value="concise">Concise (summarize)</option>
      </select>
    </label>
  </div>
</section>

<style>
  @import "./tab.css";
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
  .action-row {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
    padding-top: 14px;
    border-top: 1px solid var(--border);
  }
  .btn-test {
    width: 100%;
    background: var(--accent);
    border: none;
    color: #fff;
    border-radius: var(--radius);
    padding: 6px 0;
    font-size: 12px;
    cursor: pointer;
    font-weight: 700;
    transition: var(--transition-snappy-fast);
    box-shadow: 0 2px 6px rgba(56, 189, 248, 0.15);
  }
  .btn-test:hover:not(:disabled) {
    filter: brightness(1.1);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(56, 189, 248, 0.3);
  }
  .btn-test:active:not(:disabled) {
    transform: translateY(0);
  }
  .btn-test:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }
  .status-msg {
    font-size: 12px;
    font-weight: 600;
    text-align: center;
  }
  .status-msg.success {
    color: #4caf50;
  }
  .status-msg.error {
    color: #e57373;
  }
</style>
