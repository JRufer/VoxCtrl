<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  interface HistoryEntry {
    text: string;
    target_id: string;
    timestamp: string;
    inference_ms: number;
  }

  let entries = $state<HistoryEntry[]>([]);
  let loading = $state(true);

  onMount(async () => {
    entries = await invoke<HistoryEntry[]>("get_history");
    loading = false;
  });

  async function clearHistory() {
    await invoke("clear_history");
    entries = [];
  }

  function formatTime(iso: string) {
    return new Date(iso).toLocaleTimeString();
  }
</script>

<div class="history-root">
  <header>
    <h1>Transcript History</h1>
    <button class="btn-clear" onclick={clearHistory}>Clear</button>
  </header>

  {#if loading}
    <div class="empty">Loading…</div>
  {:else if entries.length === 0}
    <div class="empty">No transcripts yet.</div>
  {:else}
    <div class="list">
      {#each entries as entry}
        <div class="entry">
          <div class="entry-text">{entry.text}</div>
          <div class="entry-meta">
            <span>{formatTime(entry.timestamp)}</span>
            <span>→ {entry.target_id}</span>
            <span>{entry.inference_ms}ms</span>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .history-root {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--text);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  h1 { font-size: 16px; font-weight: 600; }

  .btn-clear {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    padding: 4px 12px;
    border-radius: var(--radius);
    font-size: 12px;
  }
  .btn-clear:hover { color: var(--accent); border-color: var(--accent); }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .entry {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px 14px;
  }

  .entry-text {
    font-size: 14px;
    line-height: 1.5;
    margin-bottom: 6px;
    white-space: pre-wrap;
  }

  .entry-meta {
    display: flex;
    gap: 12px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .empty {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
  }
</style>
