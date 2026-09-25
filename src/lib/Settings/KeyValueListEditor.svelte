<!--
  An editable list of word → replacement pairs, bound to a string map in the
  config. Used for Features → Snippets and TTS → Pronunciation snippets.

  Rows are edited as a list (so a half-typed row, or two rows mid-edit with the
  same key, are allowed while typing) and written back to `value` as a map:
  blank keys are skipped and keys/values trimmed. `onchange` fires only for
  edits made here, not for the initial normalization.
-->
<script lang="ts">
  let {
    value = $bindable(),
    title,
    hint,
    addLabel,
    keyPlaceholder,
    valuePlaceholder,
    emptyText,
    class: className = "",
    onchange,
  }: {
    value: Record<string, string> | undefined;
    title: string;
    hint: string;
    addLabel: string;
    keyPlaceholder: string;
    valuePlaceholder: string;
    emptyText: string;
    class?: string;
    onchange?: () => void;
  } = $props();

  let rows = $state<{ key: string; val: string }[]>(
    Object.entries(value || {}).map(([k, v]) => ({ key: k, val: v as string })),
  );

  let initialized = false;
  $effect(() => {
    const next: Record<string, string> = {};
    for (const { key, val } of rows) {
      if (key.trim()) {
        next[key.trim()] = val.trim();
      }
    }

    const existing = value || {};
    const existingKeys = Object.keys(existing);
    const nextKeys = Object.keys(next);
    const changed =
      existingKeys.length !== nextKeys.length || nextKeys.some((k) => existing[k] !== next[k]);

    if (changed) {
      value = next;
      if (initialized) {
        onchange?.();
      }
    }
    initialized = true;
  });

  function addRow() {
    rows = [...rows, { key: "", val: "" }];
  }

  function removeRow(index: number) {
    rows = rows.filter((_, i) => i !== index);
  }
</script>

<div class={["field-group", className].filter(Boolean).join(" ")}>
  <div class="field-label-row">
    <div style="display: flex; flex-direction: column;">
      <h3 style="margin-bottom: 0;">{title}</h3>
      <p class="hint" style="margin-top: 4px;">{hint}</p>
    </div>
    <button class="btn-add-inline" type="button" onclick={addRow}>
      ＋ {addLabel}
    </button>
  </div>

  <div class="dynamic-list">
    {#each rows as row, idx}
      <div class="dynamic-list-row">
        <input
          type="text"
          placeholder={keyPlaceholder}
          bind:value={rows[idx].key}
          style="flex: 0.4;"
        />
        <span style="color: var(--text-muted);">→</span>
        <input
          type="text"
          placeholder={valuePlaceholder}
          bind:value={rows[idx].val}
          style="flex: 1;"
        />
        <button class="btn-remove-inline" type="button" onclick={() => removeRow(idx)}>✕</button>
      </div>
    {/each}
    {#if rows.length === 0}
      <div class="empty-state" style="padding: 20px; grid-column: 1 / -1;">
        <p>{emptyText}</p>
      </div>
    {/if}
  </div>
</div>
