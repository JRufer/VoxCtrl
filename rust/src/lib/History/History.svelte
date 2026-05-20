<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";

  interface HistoryEntry {
    text: string;
    target_id: string;
    timestamp: string;
    inference_ms: number;
  }

  interface OutputTarget {
    id: string;
    label: string;
    delivery: string;
  }

  let entries = $state<HistoryEntry[]>([]);
  let targets = $state<OutputTarget[]>([]);
  let loading = $state(true);

  // Quick lookup map for target details
  let targetMap = $derived(new Map(targets.map(t => [t.id, t])));

  async function loadHistory() {
    try {
      const fetchedHistory = await invoke<HistoryEntry[]>("get_history");
      entries = fetchedHistory.slice(0, 100);
    } catch (err) {
      console.error("Failed to load history", err);
    }
  }

  onMount(async () => {
    try {
      // Run history and target fetches concurrently
      const [fetchedHistory, fetchedTargets] = await Promise.all([
        invoke<HistoryEntry[]>("get_history"),
        invoke<OutputTarget[]>("get_targets")
      ]);
      entries = fetchedHistory.slice(0, 100);
      targets = fetchedTargets;
    } catch (err) {
      console.error("Failed to load history or targets", err);
    } finally {
      loading = false;
    }

    // Reactively refresh when window is focused or when backend status updates are dispatched
    const unlistenFocus = await listen("tauri://focus", () => {
      loadHistory();
    });

    const unlistenStatus = await listen("status-tick", () => {
      loadHistory();
    });

    return () => {
      unlistenFocus();
      unlistenStatus();
    };
  });

  async function clearHistory() {
    await invoke("clear_history");
    entries = [];
  }

  function formatTime(iso: string) {
    try {
      const d = new Date(iso);
      if (isNaN(d.getTime())) return "Unknown Time";
      // Format as e.g. "May 20, 22:15:30"
      return d.toLocaleString(undefined, {
        month: 'short',
        day: 'numeric',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit'
      });
    } catch {
      return "Unknown Time";
    }
  }

  function deliveryLabel(d: string) {
    const map: Record<string, string> = {
      inject: "Inject",
      clipboard: "Clipboard",
      exec: "Exec",
      pipe: "Pipe",
      socket: "Socket",
      file: "File",
      dbus: "DBus",
      http: "HTTP",
      webhook: "Webhook",
    };
    return map[d] ?? d;
  }

  function getTargetNotes(targetId: string) {
    const target = targetMap.get(targetId);
    if (!target) {
      return targetId === "default" ? "Focused Window (Inject)" : targetId;
    }
    return `${target.label} (${deliveryLabel(target.delivery)})`;
  }
</script>

<div class="history-root">
  <header>
    <div class="title-section">
      <span class="title-icon">📋</span>
      <h1>Transcript History</h1>
    </div>
    <button class="btn-clear" onclick={clearHistory}>Clear History</button>
  </header>

  {#if loading}
    <div class="empty">
      <span class="spinner">⏳</span>
      <span>Loading history...</span>
    </div>
  {:else if entries.length === 0}
    <div class="empty">
      <span class="empty-icon">📭</span>
      <span>No transcripts yet this session.</span>
    </div>
  {:else}
    <div class="list">
      {#each entries as entry}
        <div class="entry">
          <div class="entry-text">{entry.text}</div>
          <div class="entry-meta">
            <span class="meta-item">
              <span class="meta-icon">⏰</span>
              {formatTime(entry.timestamp)}
            </span>
            <span class="meta-item">
              <span class="meta-icon">🎯</span>
              {getTargetNotes(entry.target_id)}
            </span>
            <span class="meta-item accent">
              <span class="meta-icon">⚡</span>
              {entry.inference_ms}ms
            </span>
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
    height: 100vh;
    background: var(--bg);
    color: var(--text);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 18px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
  }

  .title-section {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .title-icon {
    font-size: 20px;
  }

  h1 {
    font-size: 17px;
    font-weight: 600;
    letter-spacing: 0.5px;
  }

  .btn-clear {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    padding: 6px 14px;
    border-radius: var(--radius);
    font-size: 12px;
    font-weight: 500;
    transition: all 0.2s ease;
  }

  .btn-clear:hover {
    color: var(--accent);
    border-color: var(--accent);
    background: rgba(233, 69, 96, 0.05);
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 20px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .entry {
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 18px;
    transition: transform 0.2s ease, border-color 0.2s ease;
  }

  .entry:hover {
    transform: translateY(-1px);
    border-color: rgba(255, 255, 255, 0.15);
  }

  .entry-text {
    font-size: 14px;
    line-height: 1.6;
    margin-bottom: 8px;
    white-space: pre-wrap;
    font-weight: 400;
  }

  .entry-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 16px;
    font-size: 11px;
    color: var(--text-muted);
  }

  .meta-item {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .meta-icon {
    opacity: 0.7;
  }

  .meta-item.accent {
    color: var(--accent2);
  }

  .empty {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-muted);
    font-size: 14px;
  }

  .empty-icon, .spinner {
    font-size: 32px;
  }
</style>
