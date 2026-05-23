<script lang="ts">
  import { status } from "../../stores/status";

  let { recording = false, speaking = false } = $props();

  let isReady = $derived($status.audio_ready !== false);
  let label = $derived(
    $status.processing ? "Processing…" : recording ? (isReady ? "Listening…" : "Connecting Mic…") : speaking ? "Speaking…" : ""
  );
  let color = $derived(
    $status.processing ? "#38bdf8" : recording ? (isReady ? "#ff6b35" : "#f59e0b") : "#10b981"
  );
</script>

<div class="voice-card-container">
  <div class="card" style="--dot-color: {color}">
    <span class="dot" class:pulse={recording || speaking || $status.processing}></span>
    <span class="label">{label}</span>
    {#if recording || speaking || $status.processing}
      <span class="target-badge">
        <span class="target-icon">🎯</span>
        <span class="target-text">{$status.active_target_label || "Focused Window"}</span>
      </span>
    {/if}
  </div>
</div>

<style>
  .voice-card-container {
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .card {
    display: flex;
    align-items: center;
    gap: 12px;
    background: rgba(13, 14, 18, 0.86);
    backdrop-filter: blur(16px);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px; /* Option C */
    padding: 10px 18px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.6);
    color: #e5e7eb;
    font-size: 13.5px;
    font-weight: 600;
    pointer-events: none;
    letter-spacing: -0.1px;
    animation: zoom-in 0.22s cubic-bezier(0.175, 0.885, 0.32, 1.25) both;
  }

  @keyframes zoom-in {
    from { opacity: 0; transform: scale(0.9) translateY(8px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--dot-color);
    flex-shrink: 0;
    box-shadow: 0 0 8px var(--dot-color);
    transition: background 0.2s;
  }

  .dot.pulse {
    animation: pulse 1s cubic-bezier(0.175, 0.885, 0.32, 1.275) infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.4; transform: scale(1.4); }
  }

  .label {
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .target-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 6px;
    padding: 2px 7px;
    color: #9ca3af;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    animation: slide-in 0.2s cubic-bezier(0.175, 0.885, 0.32, 1.2) both;
    flex-shrink: 0;
  }

  .target-icon {
    font-size: 10px;
    line-height: 1;
  }

  .target-text {
    max-width: 110px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes slide-in {
    from { opacity: 0; transform: translateX(6px); }
    to   { opacity: 1; transform: translateX(0); }
  }
</style>
