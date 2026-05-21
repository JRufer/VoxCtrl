<script lang="ts">
  import { status } from "../../stores/status";
  let { recording = false } = $props();
  let isReady = $derived($status.audio_ready !== false);
</script>

<div class="pulse-outer">
  <div class="pulse-row">
    <div class="pulse-container" class:active={recording && isReady} class:initializing={recording && !isReady} class:processing={$status.processing}>
      <div class="ring ring1"></div>
      <div class="ring ring2"></div>
      <div class="core"></div>
    </div>
    {#if recording || $status.processing}
      {@const badgeColor = $status.processing ? '#00e5ff' : (isReady ? '#4fc3f7' : '#ff9100')}
      {@const badgeBorder = $status.processing ? 'rgba(0, 229, 255, 0.35)' : (isReady ? 'rgba(79, 195, 247, 0.35)' : 'rgba(255, 145, 0, 0.35)')}
      <span class="target-badge" style="border-color: {badgeBorder}; color: {badgeColor};">
        <span class="target-icon">{$status.processing ? "🧠" : (isReady ? "🎯" : "⏳")}</span>
        <span class="target-text">
          {#if $status.processing}
            Processing...
          {:else if isReady}
            {$status.active_target_label || "Focused Window"}
          {:else}
            Connecting Mic...
          {/if}
        </span>
      </span>
    {/if}
  </div>
</div>

<style>
  .pulse-outer {
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .pulse-row {
    display: flex;
    align-items: center;
    gap: 12px;
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

  .pulse-container {
    position: relative;
    width: 80px;
    height: 80px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .ring {
    position: absolute;
    border-radius: 50%;
    border: 2px solid var(--recording-color, #e94560);
    opacity: 0;
  }

  .active .ring {
    animation: expand 2s ease-out infinite;
  }

  .initializing .ring {
    border-color: #ff9100;
    animation: expand 3.5s ease-in-out infinite;
  }

  .ring1 { width: 100%; height: 100%; }
  .ring2 { width: 100%; height: 100%; animation-delay: 1s !important; }
  .initializing .ring2 { animation-delay: 1.75s !important; }

  .core {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--recording-color, #e94560);
    transition: background 0.3s;
  }

  .initializing .core {
    background: #ff9100;
    box-shadow: 0 0 10px #ff9100;
  }

  .processing .ring {
    border-color: #00e5ff;
    animation: expand 2.2s ease-out infinite;
  }

  .processing .ring2 {
    animation-delay: 1.1s !important;
  }

  .processing .core {
    background: linear-gradient(to bottom right, #00e5ff, #7c4dff);
    box-shadow: 0 0 12px #00e5ff;
  }

  @keyframes expand {
    0%   { opacity: 0.8; transform: scale(0.3); }
    100% { opacity: 0;   transform: scale(1.5); }
  }
</style>
