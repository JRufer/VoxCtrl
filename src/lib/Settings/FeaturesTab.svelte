<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  if (cfg.engine && !cfg.engine.s1_mini) {
    cfg.engine.s1_mini = { enabled: false, styling: "semi-formal" };
  }
  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  // ── S1-mini Dictation Cleanup ────────────────────────────────────────────
  let s1MiniDownloaded = $state(false);
  let s1MiniChecking = $state(false);
  let s1MiniDownloading = $state(false);
  let s1MiniGpu = $state<string | null>(null);

  const GPU_LABELS: Record<string, string> = {
    cuda: "CUDA (NVIDIA)",
    vulkan: "Vulkan (AMD/Intel/NVIDIA)",
    coreml: "CoreML (Apple)",
    webgpu: "WebGPU (AMD/Intel/NVIDIA)",
  };
  const gpuLabel = (id: string) => GPU_LABELS[id] ?? id;

  function ensureS1MiniConfig() {
    if (!cfg.engine.s1_mini) {
      cfg.engine.s1_mini = {
        enabled: false,
        styling: "semi-formal",
      };
    }
  }

  async function checkS1MiniDownloaded() {
    s1MiniChecking = true;
    try {
      s1MiniDownloaded = await invoke<boolean>("check_s1_mini_downloaded");
    } catch (e) {
      console.error("Failed to check S1-mini download status", e);
      s1MiniDownloaded = false;
    } finally {
      s1MiniChecking = false;
    }
  }

  async function triggerS1MiniDownload() {
    if (s1MiniDownloading) return;
    s1MiniDownloading = true;
    try {
      await invoke("download_s1_mini_model");
      s1MiniDownloaded = true;
    } catch (e) {
      alert(`Failed to download S1-mini model: ${e}`);
    } finally {
      s1MiniDownloading = false;
    }
  }

  async function onS1MiniToggle() {
    ensureS1MiniConfig();
    markDirty();
    if (cfg.engine.s1_mini.enabled && !s1MiniDownloaded) {
      await triggerS1MiniDownload();
    }
  }

  let s1MiniEnabled = $derived(!!cfg.engine.s1_mini?.enabled);

  onMount(async () => {
    ensureS1MiniConfig();
    checkS1MiniDownloaded();
    try {
      const support = await invoke<{ s1_mini_gpu: string | null }>("accelerator_support");
      s1MiniGpu = support.s1_mini_gpu ?? null;
    } catch (e) {
      console.error("Failed to query GPU support", e);
    }
  });

  // Snippets editing
  let snippetList = $state<{key: string, val: string}[]>(
    Object.entries(cfg.features.snippets).map(([k, v]) => ({ key: k, val: v as string }))
  );

  let isSnippetInitialized = false;
  $effect(() => {
    const list = snippetList;
    const newSnippets: Record<string, string> = {};
    for (const {key, val} of list) {
      if (key.trim()) {
        newSnippets[key.trim()] = val.trim();
      }
    }

    const existing = cfg.features.snippets || {};
    const existingKeys = Object.keys(existing);
    const newKeys = Object.keys(newSnippets);
    let changed = existingKeys.length !== newKeys.length;
    if (!changed) {
      for (const k of newKeys) {
        if (existing[k] !== newSnippets[k]) {
          changed = true;
          break;
        }
      }
    }

    if (changed) {
      cfg.features.snippets = newSnippets;
      if (isSnippetInitialized) {
        markDirty();
      }
    }
    isSnippetInitialized = true;
  });

  function addEmptySnippetRow() {
    snippetList = [...snippetList, { key: "", val: "" }];
  }

  function removeSnippetRow(index: number) {
    snippetList = snippetList.filter((_, i) => i !== index);
  }

  let customVocabString = $derived(
    cfg.features.custom_vocabulary ? cfg.features.custom_vocabulary.join(", ") : ""
  );

  function onCustomVocabChange(e: Event) {
    const target = e.target as HTMLTextAreaElement;
    cfg.features.custom_vocabulary = target.value
      .split(",")
      .map(w => w.trim())
      .filter(w => w.length > 0);
    markDirty();
  }

  // Reusable Svelte action to auto-resize textareas dynamically to fit their contents
  function autoResize(node: HTMLTextAreaElement) {
    function resize() {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight}px`;
    }
    node.addEventListener("input", resize);
    // Initial calculation on mount or state update
    const timer = setTimeout(resize, 0);

    return {
      update() {
        resize();
      },
      destroy() {
        clearTimeout(timer);
        node.removeEventListener("input", resize);
      }
    };
  }
</script>

<section>
  <h2>Post-Processing</h2>

  <div class="field-group">
    <div class="field-label-row">
      <h3>S1-mini Dictation Cleanup</h3>
      {#if s1MiniDownloaded}
        <span class="status-pill success">✔ Ready {s1MiniGpu ? `(${gpuLabel(s1MiniGpu)})` : "(CPU)"}</span>
      {:else if s1MiniDownloading}
        <span class="status-pill downloading">⏳ Downloading</span>
      {:else if s1MiniEnabled}
        <span class="status-pill error">Missing</span>
      {/if}
    </div>

    <label class="field">
      <div class="field-title-col">
        <span>Enable S1-mini dictation cleanup</span>
        <p class="hint">
          Uses Superwhisper's local Qwen-based text normalizer to clean speech-to-text transcripts into natural punctuation, casing, and spoken corrections while strictly preserving voice commands.
        </p>
      </div>
      <input
        type="checkbox"
        bind:checked={cfg.engine.s1_mini.enabled}
        onchange={onS1MiniToggle}
      />
    </label>

    <div class="s1-mini-note">
      Note: S1-Mini is a ~480 MB download.
    </div>

    {#if s1MiniEnabled}
      <div class="model-status-container mt-1">
        {#if s1MiniChecking}
          <span class="status-checking">⏳ Checking S1-mini model files...</span>
        {:else if s1MiniDownloading}
          <span class="status-downloading"
            >⏳ Downloading S1-mini model (s1-mini-q4_k_m.gguf & tokenizer.json)...</span
          >
        {:else if s1MiniDownloaded}
          <span class="status-downloaded">✔ Model downloaded and ready {s1MiniGpu ? `(${gpuLabel(s1MiniGpu)} GPU accelerated)` : "on CPU"}</span>
        {:else}
          <div class="status-missing-wrapper">
            <span class="status-missing">❌ Model files missing</span>
            <button
              class="btn-download"
              type="button"
              onclick={triggerS1MiniDownload}
            >
              📥 Download S1-mini
            </button>
          </div>
        {/if}
      </div>

      <label class="field">
        <span>Cleanup styling</span>
        <CustomSelect
          bind:value={cfg.engine.s1_mini.styling}
          options={[
            { value: "semi-formal", label: "Semi-formal (Default)" },
            { value: "casual", label: "Casual" },
            { value: "formal", label: "Formal" },
            { value: "verbatim", label: "Verbatim" },
            { value: "concise", label: "Concise" }
          ]}
          onchange={markDirty}
        />
      </label>
    {/if}
  </div>

  <div class="field-group" class:disabled-section={s1MiniEnabled}>
    <h3>Basic Text Cleanup</h3>
    {#if s1MiniEnabled}
      <p class="hint disabled-note">
        These features are covered by S1-mini when enabled.
      </p>
    {/if}
    <label class="field">
      <span>Remove filler words (uh, um, hmm…)</span>
      <input type="checkbox" bind:checked={cfg.features.remove_fillers} onchange={markDirty} disabled={s1MiniEnabled} />
    </label>
    <label class="field">
      <span>Spoken punctuation ("period" → ".")</span>
      <input type="checkbox" bind:checked={cfg.features.spoken_punctuation} onchange={markDirty} disabled={s1MiniEnabled} />
    </label>
    <label class="field">
      <span>Auto-format lists ("first, second, third")</span>
      <input type="checkbox" bind:checked={cfg.features.auto_format_lists} onchange={markDirty} disabled={s1MiniEnabled} />
    </label>
  </div>

  <div class="field-group">
    <h3>Custom Dictionary</h3>
    <p class="hint">Provide a comma-separated list of words (e.g. names or jargon like "Waylin, Rufer, Enola, Kenz") that are hard to spell. The transcription process will correct these in the final text.</p>
    <textarea 
      class="custom-vocab-input"
      placeholder="e.g. Waylin, Rufer, Enola, Kenz"
      value={customVocabString}
      oninput={onCustomVocabChange}
      use:autoResize
    ></textarea>
  </div>

  <div class="field-group">
    <div class="field-label-row">
      <div style="display: flex; flex-direction: column;">
        <h3 style="margin-bottom: 0;">Snippets</h3>
        <p class="hint" style="margin-top: 4px;">Type a trigger word → it expands to the replacement text.</p>
      </div>
      <button class="btn-add-inline" type="button" onclick={addEmptySnippetRow}>
        ＋ Add Snippet
      </button>
    </div>

    <div class="dynamic-list">
      {#each snippetList as snippet, idx}
        <div class="dynamic-list-row">
          <input 
            type="text" 
            placeholder="Trigger word" 
            bind:value={snippetList[idx].key} 
            style="flex: 0.4;"
          />
          <span style="color: var(--text-muted);">→</span>
          <input 
            type="text" 
            placeholder="Expansion text" 
            bind:value={snippetList[idx].val} 
            style="flex: 1;"
          />
          <button class="btn-remove-inline" type="button" onclick={() => removeSnippetRow(idx)}>✕</button>
        </div>
      {/each}
      {#if snippetList.length === 0}
        <div class="empty-state" style="padding: 20px; grid-column: 1 / -1;">
          <p>No snippets defined.</p>
        </div>
      {/if}
    </div>
  </div>
</section>

<style>
  @reference "../../app.css";

  .custom-vocab-input {
    @apply w-full min-h-[80px] bg-[var(--bg)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 px-3 text-[13px] resize-y mt-2 outline-none box-border transition-all duration-200 ease-out;
  }

  .custom-vocab-input:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(79,195,247,0.2)];
  }

  .custom-vocab-input::placeholder {
    @apply text-[var(--text-muted)] opacity-50;
  }

  .model-status-container {
    @apply flex items-center bg-[var(--bg)] border border-[var(--border)] rounded-[var(--radius)] p-2.5 px-3.5 text-[13px] min-h-[42px] mb-3;
  }
  .status-downloaded {
    @apply text-emerald-400 font-semibold;
  }
  .status-downloading {
    @apply text-[var(--accent2)];
  }
  .status-checking {
    @apply text-[var(--text-muted)];
  }
  .status-missing-wrapper {
    @apply flex items-center justify-between w-full;
  }
  .status-missing {
    @apply text-red-400;
  }
  .btn-download {
    @apply bg-[var(--accent)] border-none text-white rounded-[var(--radius)] p-1.5 px-3 text-xs cursor-pointer font-semibold transition-colors duration-200;
  }
  .btn-download:hover {
    @apply bg-[var(--accent2)];
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
  .status-pill.downloading {
    @apply bg-cyan-500/15 text-cyan-300 border border-cyan-500/30;
  }

  .field-title-col {
    @apply flex flex-col flex-1 mr-4;
  }
  .field-title-col span {
    @apply text-[13px] font-medium text-[var(--color-obsidian-100)];
  }
  .field-title-col .hint {
    @apply text-xs text-[var(--text-muted)] mt-0.5 leading-relaxed;
  }
  .s1-mini-note {
    @apply text-[11.5px] font-medium text-[var(--color-accent-blue)] opacity-90 -mt-1;
  }

  .disabled-section {
    @apply opacity-50 pointer-events-none;
  }
  .disabled-note {
    @apply text-[var(--color-accent-blue)] opacity-90;
  }
</style>
