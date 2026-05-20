<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();
  function markDirty() { configDirty.set(true); }

  // Snippets editing
  let snippetKey = $state("");
  let snippetVal = $state("");
  let snippetEntries = $derived(Object.entries(cfg.features.snippets));

  function addSnippet() {
    if (!snippetKey.trim() || !snippetVal.trim()) return;
    cfg.features.snippets = { ...cfg.features.snippets, [snippetKey.trim()]: snippetVal.trim() };
    snippetKey = "";
    snippetVal = "";
    markDirty();
  }

  function removeSnippet(key: string) {
    const { [key]: _, ...rest } = cfg.features.snippets;
    cfg.features.snippets = rest;
    markDirty();
  }
</script>

<section>
  <h2>Features & Post-Processing</h2>

  <div class="field-group">
    <h3>Text cleanup</h3>
    <label class="field">
      <span>Remove filler words (uh, um, hmm…)</span>
      <input type="checkbox" bind:checked={cfg.features.remove_fillers} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Spoken punctuation ("period" → ".")</span>
      <input type="checkbox" bind:checked={cfg.features.spoken_punctuation} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Auto-format lists ("first, second, third")</span>
      <input type="checkbox" bind:checked={cfg.features.auto_format_lists} onchange={markDirty} />
    </label>
  </div>

  <div class="field-group">
    <h3>Snippets</h3>
    <p class="hint">Type a trigger word → it expands to the replacement text.</p>

    {#each snippetEntries as [key, val]}
      <div class="snippet-row">
        <code>{key}</code>
        <span>→</span>
        <span class="snippet-val">{val}</span>
        <button class="btn-remove" onclick={() => removeSnippet(key)}>✕</button>
      </div>
    {/each}

    <div class="snippet-add">
      <input type="text" placeholder="trigger" bind:value={snippetKey} />
      <span>→</span>
      <input type="text" placeholder="expansion" bind:value={snippetVal} />
      <button class="btn-add" onclick={addSnippet}>Add</button>
    </div>
  </div>
</section>

<style>
  @import "./tab.css";
  .snippet-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
  }
  .snippet-val { flex: 1; color: var(--text-muted); }
  .btn-remove {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 12px;
    padding: 0 4px;
  }
  .btn-remove:hover { color: var(--accent); }
  .snippet-add {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }
  .snippet-add input { flex: 1; }
  .btn-add {
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: 4px;
    padding: 4px 10px;
    font-size: 12px;
  }
</style>
