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
    <h3>Custom Dictionary</h3>
    <p class="hint">Provide a comma-separated list of words (e.g. names or jargon like "Waylin, Rufer, Enola, Kenz") that are hard to spell. The transcription process will correct these in the final text.</p>
    <textarea 
      class="custom-vocab-input"
      placeholder="e.g. Waylin, Rufer, Enola, Kenz"
      value={customVocabString}
      oninput={onCustomVocabChange}
    ></textarea>
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

  .snippet-val {
    flex: 1;
    color: var(--text-muted);
  }

  .btn-remove {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 12px;
    padding: 0 4px;
    cursor: pointer;
    transition: color 0.15s ease;
  }

  .btn-remove:hover {
    color: var(--accent);
  }

  .snippet-add {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 4px;
  }

  .snippet-add input {
    flex: 1;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 8px 12px;
    font-size: 13px;
    min-width: 0;
    outline: none;
    box-sizing: border-box;
    transition: all 0.2s ease;
  }

  .snippet-add input:focus {
    border-color: var(--accent2);
    box-shadow: 0 0 0 2px rgba(79, 195, 247, 0.2);
  }

  .snippet-add input::placeholder {
    color: var(--text-muted);
    opacity: 0.5;
  }

  .btn-add {
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    padding: 8px 16px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s ease;
  }

  .btn-add:hover {
    opacity: 0.95;
    transform: translateY(-1px);
  }

  .btn-add:active {
    transform: translateY(0);
  }

  .custom-vocab-input {
    width: 100%;
    min-height: 80px;
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 8px 12px;
    font-family: inherit;
    font-size: 13px;
    resize: vertical;
    margin-top: 8px;
    outline: none;
    box-sizing: border-box;
    transition: all 0.2s ease;
  }

  .custom-vocab-input:focus {
    border-color: var(--accent2);
    box-shadow: 0 0 0 2px rgba(79, 195, 247, 0.2);
  }

  .custom-vocab-input::placeholder {
    color: var(--text-muted);
    opacity: 0.5;
  }
</style>
