<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
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
  @reference "tailwindcss";

  .field.col {
    @apply flex-col items-start gap-1.5;
  }
  textarea {
    @apply w-full bg-[var(--bg)] border border-[var(--border)] rounded p-2 text-[var(--text)] text-[13px] font-mono resize-y;
  }
  .action-row {
    @apply flex flex-col items-center gap-2 mt-3.5 pt-3.5 border-t border-[var(--border)];
  }
  .btn-test {
    @apply w-full bg-[var(--accent)] border-none text-white rounded-[var(--radius)] py-1.5 text-xs cursor-pointer font-bold transition-all duration-150 ease-out shadow-[0_2px_6px_rgba(56,189,248,0.15)];
  }
  .btn-test:hover:not(:disabled) {
    @apply brightness-110 -translate-y-0.5 shadow-[0_4px_12px_rgba(56,189,248,0.3)];
  }
  .btn-test:active:not(:disabled) {
    @apply translate-y-0;
  }
  .btn-test:disabled {
    @apply opacity-60 cursor-not-allowed;
  }
  .status-msg {
    @apply text-xs font-semibold text-center;
  }
  .status-msg.success {
    @apply text-emerald-400;
  }
  .status-msg.error {
    @apply text-red-400;
  }
</style>
