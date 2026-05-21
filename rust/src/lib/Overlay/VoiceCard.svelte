<script lang="ts">
  import { status } from "../../stores/status";

  let { recording = false, speaking = false } = $props();

  let isReady = $derived($status.audio_ready !== false);
  let label = $derived(
    $status.processing ? "Processing…" : recording ? (isReady ? "Listening…" : "Connecting Mic…") : speaking ? "Speaking…" : ""
  );
  let color = $derived(
    $status.processing ? "#00e5ff" : recording ? (isReady ? "#e94560" : "#ff9100") : "#4fc3f7"
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
    gap: 10px;
    background: rgba(15, 15, 25, 0.88);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 24px;
    padding: 12px 20px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.5);
    color: #e0e0e0;
    font-size: 15px;
    font-weight: 500;
    pointer-events: none;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--dot-color);
    flex-shrink: 0;
  }

  .dot.pulse {
    animation: pulse 1.2s ease-in-out infinite;
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; transform: scale(1); }
    50% { opacity: 0.5; transform: scale(1.3); }
  }

  .target-badge {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: rgba(15, 34, 62, 0.92);
    border: 1px solid rgba(79, 195, 247, 0.35);
    border-radius: 6px;
    padding: 2px 7px;
    color: #4fc3f7;
    font-size: 9px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    animation: slide-in 0.2s ease both;
    flex-shrink: 0;
  }

  .target-icon {
    font-size: 10px;
    line-height: 1;
  }

  .target-text {
    max-width: 120px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes slide-in {
    from { opacity: 0; transform: translateX(4px); }
    to   { opacity: 1; transform: translateX(0); }
  }
</style>
