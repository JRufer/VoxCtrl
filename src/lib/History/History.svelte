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
      mcp: "MCP",
    };
    return map[d] ?? d;
  }

  function getTargetNotes(targetId: string) {
    if (!targetId) return "";
    const ids = targetId.split(',').map(s => s.trim()).filter(s => s.length > 0);
    return ids.map(id => {
      const target = targetMap.get(id);
      if (!target) {
        return id === "default" ? "Focused Window (Inject)" : id;
      }
      return `${target.label} (${deliveryLabel(target.delivery)})`;
    }).join(" + ");
  }
</script>

<div class="history-root">
  <header class="toolbar">
    <div class="title-section">
      <span class="title-logo">📋</span>
      <div class="title-text">
        <h1>Transcript History</h1>
        <p class="subtitle">DICTATION LOGS FOR THIS SESSION</p>
      </div>
    </div>
    <button class="btn-clear interactive-spring" onclick={clearHistory}>
      🧹 Clear Log
    </button>
  </header>

  {#if loading}
    <div class="empty-state">
      <div class="spinner-container">
        <svg class="spinner-svg" viewBox="0 0 50 50">
          <circle class="path" cx="25" cy="25" r="20" fill="none" stroke-width="5"></circle>
        </svg>
      </div>
      <span class="loading-label">Retrieving transcript log...</span>
    </div>
  {:else if entries.length === 0}
    <div class="empty-state">
      <div class="empty-graphic">
        <span class="empty-emoji">📭</span>
      </div>
      <h3 class="empty-title">Log is Empty</h3>
      <p class="empty-subtitle">Your transcribed speech sessions will appear here.</p>
    </div>
  {:else}
    <div class="list">
      {#each entries as entry}
        <div class="entry card-spring">
          <div class="entry-bubble">
            <span class="bubble-decorator"></span>
            <p class="entry-text">{entry.text}</p>
          </div>
          
          <div class="entry-meta">
            <!-- Time Badge -->
            <span class="badge badge-neutral">
              <span class="badge-icon">⏰</span>
              {formatTime(entry.timestamp)}
            </span>
            
            <!-- Target Badge -->
            <span class="badge badge-blue">
              <span class="badge-icon">🎯</span>
              {getTargetNotes(entry.target_id)}
            </span>
            
            <!-- Inference Speed Badge -->
            <span class="badge badge-orange">
              <span class="badge-icon">⚡</span>
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
    width: 100vw;
    background: var(--bg);
    color: var(--text);
    overflow: hidden;
  }

  /* Redesigned Premium Toolbar */
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--color-obsidian-900);
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.2);
    z-index: 10;
  }

  .title-section {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .title-logo {
    font-size: 24px;
    filter: drop-shadow(0 2px 8px rgba(56, 189, 248, 0.25));
  }

  .title-text {
    display: flex;
    flex-direction: column;
  }

  h1 {
    font-size: 16px;
    font-weight: 850;
    color: #fff;
    letter-spacing: -0.5px;
    line-height: 1.1;
  }

  .subtitle {
    font-size: 8px;
    font-weight: 700;
    color: var(--color-obsidian-300);
    letter-spacing: 0.12em;
    margin-top: 2px;
  }

  /* Clear button with spring active state */
  .btn-clear {
    background: rgba(255, 107, 53, 0.08);
    border: 1px solid rgba(255, 107, 53, 0.2);
    color: var(--color-accent-tangerine);
    padding: 6px 14px;
    border-radius: var(--radius);
    font-size: 12px;
    font-weight: 750;
    transition: var(--transition-snappy-fast);
  }

  .btn-clear:hover {
    color: #fff;
    background: var(--color-accent-tangerine);
    border-color: var(--color-accent-tangerine);
    box-shadow: 0 4px 12px rgba(255, 107, 53, 0.25);
  }

  /* Scrollable Transcript Feed */
  .list {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  /* Redesigned Transcript Cards with snappy spring physics */
  .entry {
    background: var(--surface2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px 20px;
    box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
    display: flex;
    flex-direction: column;
    gap: 14px;
    animation: cardSlide 0.25s cubic-bezier(0.175, 0.885, 0.32, 1.2) forwards;
  }

  @keyframes cardSlide {
    from { opacity: 0; transform: translateY(12px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .card-spring {
    transition: var(--transition-snappy);
  }

  .entry:hover {
    transform: scale(1.01) translateY(-2px);
    border-color: rgba(255, 255, 255, 0.08);
    background-color: var(--color-obsidian-700);
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.25);
  }

  .entry-bubble {
    position: relative;
    padding-left: 10px;
  }

  .bubble-decorator {
    position: absolute;
    left: 0;
    top: 4px;
    bottom: 4px;
    width: 3px;
    background: var(--color-accent-blue);
    border-radius: 99px;
    opacity: 0.5;
  }

  .entry-text {
    font-size: 13.5px;
    line-height: 1.6;
    color: var(--color-obsidian-100);
    font-weight: 500;
    white-space: pre-wrap;
  }

  .entry:hover .entry-text {
    color: #fff;
  }

  /* Badge System */
  .entry-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .badge {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px;
    border-radius: 99px;
    font-size: 11px;
    font-weight: 700;
    letter-spacing: -0.1px;
    border: 1px solid transparent;
  }

  .badge-icon {
    font-size: 11px;
  }

  .badge-neutral {
    background: var(--color-obsidian-950);
    color: var(--color-obsidian-300);
    border-color: var(--border);
  }

  .badge-blue {
    background: rgba(56, 189, 248, 0.08);
    color: var(--color-accent-blue);
    border-color: rgba(56, 189, 248, 0.15);
  }

  .badge-orange {
    background: rgba(255, 107, 53, 0.08);
    color: var(--color-accent-tangerine);
    border-color: rgba(255, 107, 53, 0.15);
  }

  /* Premium loading & empty states */
  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--color-obsidian-300);
    padding: 40px;
  }

  .spinner-container {
    width: 44px;
    height: 44px;
    margin-bottom: 8px;
  }

  .spinner-svg {
    animation: rotate 1.8s linear infinite;
    width: 100%;
    height: 100%;
  }

  .spinner-svg .path {
    stroke: var(--color-accent-blue);
    stroke-linecap: round;
    animation: dash 1.5s ease-in-out infinite;
  }

  @keyframes rotate {
    100% { transform: rotate(360deg); }
  }

  @keyframes dash {
    0% { stroke-dasharray: 1, 150; stroke-dashoffset: 0; }
    50% { stroke-dasharray: 90, 150; stroke-dashoffset: -35; }
    100% { stroke-dasharray: 90, 150; stroke-dashoffset: -124; }
  }

  .loading-label {
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.05em;
  }

  .empty-graphic {
    width: 72px;
    height: 72px;
    background: var(--color-obsidian-900);
    border: 1px solid var(--border);
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 12px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.2);
  }

  .empty-emoji {
    font-size: 32px;
  }

  .empty-title {
    font-size: 16px;
    font-weight: 750;
    color: #fff;
    letter-spacing: -0.2px;
  }

  .empty-subtitle {
    font-size: 12px;
    color: var(--color-obsidian-300);
    text-align: center;
    max-width: 250px;
    line-height: 1.5;
  }
</style>
