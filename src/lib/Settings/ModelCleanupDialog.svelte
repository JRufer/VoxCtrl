<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig } from "../../stores/config";
  import { config, saveConfig } from "../../stores/config";
  import CustomSelect from "./CustomSelect.svelte";
  import type { RemoteFallback } from "./remote-fallback";
  import {
    formatBytes,
    keptItems,
    removableItems,
    resultSummary,
    selectedBytes,
    type CleanupPlan,
    type CleanupResult,
  } from "./model-cleanup";

  let {
    cfg = $bindable(),
    fallbackChoices,
    onClose,
  }: {
    cfg: AppConfig;
    /** The fallback dropdown's options, as the Engine tab shows them. */
    fallbackChoices: { value: RemoteFallback; label: string }[];
    onClose: () => void;
  } = $props();

  // With a server doing the transcription, the one speech model worth keeping
  // is the fallback's, so the dialog asks which that is before listing anything.
  let usesServer = $derived(cfg.engine.backend === "remote-openai");
  let fallback = $state<RemoteFallback>(cfg.engine.remote_openai?.fallback ?? "none");

  let plan = $state<CleanupPlan | null>(null);
  let selected = $state<Set<string>>(new Set());
  let loading = $state(true);
  let running = $state(false);
  let error = $state<string | null>(null);
  let result = $state<CleanupResult | null>(null);

  let removable = $derived(removableItems(plan));
  let kept = $derived(keptItems(plan));
  let freeing = $derived(selectedBytes(plan, selected));
  let summary = $derived(result ? resultSummary(result) : null);

  async function loadPlan() {
    loading = true;
    error = null;
    try {
      const next = await invoke<CleanupPlan>("plan_model_cleanup", { fallback: usesServer ? fallback : "none" });
      plan = next;
      // Everything that can go starts ticked: clearing space is why they came.
      selected = new Set(removableItems(next).map((item) => item.id));
    } catch (e) {
      error = String(e);
      plan = null;
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    // The list is worked out from the saved settings, so write any change
    // still waiting on the auto-save before asking for it.
    try {
      await saveConfig(cfg);
    } catch (e) {
      error = String(e);
      loading = false;
      return;
    }
    await loadPlan();
  });

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
  }

  async function run() {
    running = true;
    error = null;
    try {
      // The backend keeps whatever the saved fallback needs, so the choice
      // made here has to be saved before anything is removed.
      if (usesServer && (cfg.engine.remote_openai.fallback ?? "none") !== fallback) {
        cfg.engine.remote_openai.fallback = fallback;
        config.set(cfg);
        await saveConfig(cfg);
      }
      result = await invoke<CleanupResult>("run_model_cleanup", { ids: [...selected] });
      await loadPlan();
    } catch (e) {
      error = String(e);
    } finally {
      running = false;
    }
  }
</script>

<div class="modal-backdrop">
  <div class="modal glass animate-fade-in" role="dialog" aria-modal="true" aria-labelledby="cleanup-title">
    <div class="modal-header">
      <h3 id="cleanup-title">Clean up models</h3>
      <button class="close-btn" onclick={onClose} aria-label="Close">✕</button>
    </div>

    <div class="modal-body">
      <p class="hint lead">
        Removes downloaded models this computer no longer uses. Custom voices are never removed, and
        neither is anything in use.
      </p>

      {#if usesServer}
        <label class="field">
          <span>Fallback engine</span>
          <CustomSelect bind:value={fallback} options={fallbackChoices} onchange={loadPlan} disabled={running} />
        </label>
        {#if fallback === "none"}
          <div class="status-pill error" data-testid="cleanup-no-fallback">
            ⚠️ With no fallback, dictation fails with an error whenever the server can't be reached.
          </div>
        {:else}
          <p class="hint">
            This engine's model is kept so dictation still works when the server can't be reached.
          </p>
        {/if}
      {/if}

      {#if summary}
        <div class="status-pill" class:success={summary.ok} class:error={!summary.ok} data-testid="cleanup-result">
          {summary.text}
        </div>
      {/if}
      {#if error}
        <div class="status-pill error" data-testid="cleanup-error">⚠️ {error}</div>
      {/if}

      {#if loading}
        <p class="hint">Looking for downloaded models…</p>
      {:else if plan}
        {#if removable.length > 0}
          <h5>Not in use</h5>
          <ul class="items" data-testid="cleanup-removable">
            {#each removable as item (item.id)}
              <li>
                <label>
                  <input
                    type="checkbox"
                    checked={selected.has(item.id)}
                    onchange={() => toggle(item.id)}
                    disabled={running}
                  />
                  <span class="name">{item.label}</span>
                  <span class="size">{formatBytes(item.bytes)}</span>
                </label>
              </li>
            {/each}
          </ul>
        {:else if !result}
          <p class="hint" data-testid="cleanup-nothing">Nothing to remove: every downloaded model is in use.</p>
        {/if}

        {#if kept.length > 0}
          <h5>Kept</h5>
          <ul class="items kept" data-testid="cleanup-kept">
            {#each kept as item (item.id)}
              <li>
                <span class="name">{item.label}</span>
                <span class="why">{item.kept_because}</span>
                <span class="size">{formatBytes(item.bytes)}</span>
              </li>
            {/each}
          </ul>
        {/if}
      {/if}
    </div>

    <div class="modal-footer">
      <button class="btn-action secondary" onclick={onClose}>{result ? "Done" : "Cancel"}</button>
      {#if removable.length > 0}
        <button
          class="btn-action danger"
          onclick={run}
          disabled={running || loading || selected.size === 0}
          data-testid="cleanup-run"
        >
          {running ? "Removing…" : `Remove ${selected.size} and free ${formatBytes(freeing)}`}
        </button>
      {/if}
    </div>
  </div>
</div>

<style lang="postcss">
  @reference "../../app.css";

  .modal-backdrop {
    @apply fixed inset-0 bg-black/60 backdrop-blur-[4px] flex items-center justify-center z-[1000];
  }
  .modal {
    @apply w-[90%] max-w-[520px] max-h-[85vh] rounded-[var(--radius)] border border-[var(--border)] flex flex-col shadow-[0_10px_30px_rgba(0,0,0,0.5)] bg-[#1e2436];
  }
  .modal-header {
    @apply flex justify-between items-center p-4 px-5 border-b border-[var(--border)];
  }
  .modal-header h3 {
    @apply m-0 text-[15px] font-bold;
  }
  .close-btn {
    @apply bg-transparent border-none text-[var(--text-muted)] text-base cursor-pointer;
  }
  .close-btn:hover {
    @apply text-[var(--text)];
  }
  .modal-body {
    @apply p-5 overflow-y-auto flex flex-col gap-3;
  }
  .modal-footer {
    @apply flex justify-end gap-2.5 p-4 px-5 border-t border-[var(--border)];
  }
  .status-pill {
    @apply text-xs px-3 py-1.5 rounded-[var(--radius)] font-medium leading-normal;
  }
  .status-pill.success {
    @apply bg-emerald-500/15 text-emerald-300 border border-emerald-500/30;
  }
  .status-pill.error {
    @apply bg-red-500/15 text-red-300 border border-red-500/30;
  }
  .lead {
    @apply mt-0;
  }
  h5 {
    @apply m-0 mt-1 text-[11px] font-bold uppercase text-[var(--accent2)];
  }
  .items {
    @apply list-none m-0 p-0 flex flex-col gap-1;
  }
  .items li,
  .items label {
    @apply flex items-center gap-2.5 text-xs;
  }
  .items label {
    @apply w-full cursor-pointer;
  }
  .items li {
    @apply py-1.5 px-2.5 rounded-[var(--radius)] bg-white/[0.03];
  }
  .name {
    @apply flex-1 text-[var(--text)];
  }
  .why {
    @apply text-[11px] text-[var(--text-muted)];
  }
  .size {
    @apply tabular-nums text-[var(--text-muted)] min-w-[64px] text-right;
  }
  .kept .name {
    @apply text-[var(--text-muted)];
  }
</style>
