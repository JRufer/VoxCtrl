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
            <button
              class="btn-copy"
              title="Copy to clipboard"
              onclick={() => navigator.clipboard.writeText(entry.text)}
            >⎘</button>
          </div>
          
          <div class="entry-meta">
            <div class="meta-row">
              <span class="badge badge-neutral">
                <span class="badge-icon">⏰</span>
                {formatTime(entry.timestamp)}
              </span>
              <span class="badge badge-orange">
                <span class="badge-icon">⚡</span>
                {entry.inference_ms}ms
              </span>
            </div>
            <span class="badge badge-blue">
              <span class="badge-icon">🎯</span>
              {getTargetNotes(entry.target_id)}
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

  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 24px;
    border-bottom: 1px solid var(--border);
    background: var(--color-obsidian-900);
    box-shadow: 4px 0 24px rgba(0, 0, 0, 0.25);
    z-index: 10;
  }

  .title-section {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  .title-logo {
    font-size: 20px;
    filter: drop-shadow(0 2px 8px rgba(56, 189, 248, 0.3));
  }

  .title-text {
    display: flex;
    flex-direction: column;
  }

  h1 {
    font-size: 15px;
    font-weight: 850;
    color: #fff;
    letter-spacing: -0.5px;
    line-height: 1.1;
  }

  .subtitle {
    font-size: 7px;
    font-weight: 700;
    color: var(--color-accent-blue);
    letter-spacing: 0.12em;
    margin-top: 1px;
  }

  .btn-clear {
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.2);
    color: #f87171;
    padding: 6px 14px;
    border-radius: var(--radius);
    font-size: 12px;
    font-weight: 750;
    transition: var(--transition-snappy-fast);
  }

  .btn-clear:hover {
    background: #ef4444;
    border-color: #ef4444;
    color: #fff;
    box-shadow: 0 4px 12px rgba(239, 68, 68, 0.25);
  }

  .btn-clear:active {
    transform: scale(0.97);
  }

  .list {
    flex: 1;
    overflow-y: auto;
    padding: 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .entry {
    background: rgba(26, 31, 46, 0.4);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    transition: all 0.2s ease-in-out;
    animation: cardSlide 0.25s cubic-bezier(0.175, 0.885, 0.32, 1.2) forwards;
  }

  @keyframes cardSlide {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .entry:hover {
    border-color: var(--accent2);
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3);
    transform: translateY(-2px);
  }

  .entry-bubble {
    position: relative;
    padding-left: 10px;
    padding-right: 28px;
  }

  .bubble-decorator {
    position: absolute;
    left: 0;
    top: 4px;
    bottom: 4px;
    width: 3px;
    background: var(--accent2);
    border-radius: 99px;
    opacity: 0.5;
  }

  .btn-copy {
    position: absolute;
    top: 0;
    right: 0;
    width: 22px;
    height: 22px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 13px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    color: var(--text-muted);
    opacity: 0;
    transition: all 0.15s ease;
    cursor: pointer;
    padding: 0;
  }

  .entry:hover .btn-copy {
    opacity: 1;
  }

  .btn-copy:hover {
    color: var(--accent2);
    background: rgba(0, 229, 255, 0.08);
    border-color: rgba(0, 229, 255, 0.2);
  }

  .btn-copy:active {
    transform: scale(0.92);
  }

  .entry-text {
    font-size: 13px;
    line-height: 1.6;
    color: var(--text);
    font-weight: 500;
    white-space: pre-wrap;
  }

  .entry:hover .entry-text {
    color: #fff;
  }

  .entry-meta {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .meta-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .badge {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px;
    border-radius: 12px;
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.02em;
    border: 1px solid transparent;
  }

  .badge-icon {
    font-size: 10px;
  }

  .badge-neutral {
    background: rgba(255, 255, 255, 0.03);
    color: var(--text-muted);
    border-color: var(--border);
  }

  .badge-blue {
    background: rgba(0, 229, 255, 0.15);
    color: #84ffff;
    border-color: rgba(0, 229, 255, 0.3);
  }

  .badge-orange {
    background: rgba(124, 77, 255, 0.15);
    color: #b388ff;
    border-color: rgba(124, 77, 255, 0.3);
  }

  .empty-state {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: var(--text-muted);
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
    color: var(--text-muted);
    letter-spacing: 0.05em;
  }

  .empty-graphic {
    width: 64px;
    height: 64px;
    background: rgba(255, 255, 255, 0.03);
    border: 1px solid var(--border);
    border-radius: 50%;
    display: flex;
    align-items: center;
    justify-content: center;
    margin-bottom: 8px;
  }

  .empty-emoji {
    font-size: 28px;
  }

  .empty-title {
    font-size: 15px;
    font-weight: 700;
    color: #fff;
    letter-spacing: -0.2px;
  }

  .empty-subtitle {
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
    max-width: 250px;
    line-height: 1.5;
  }
</style>
