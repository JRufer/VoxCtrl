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
      {@const badgeColor = $status.processing ? '#38bdf8' : (isReady ? '#ff6b35' : '#f59e0b')}
      {@const badgeBorder = $status.processing ? 'rgba(56, 189, 248, 0.25)' : (isReady ? 'rgba(255, 107, 53, 0.25)' : 'rgba(245, 158, 11, 0.25)')}
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
    gap: 14px;
    background: rgba(13, 14, 18, 0.92);
    border: 1px solid rgba(255, 255, 255, 0.06);
    padding: 6px 16px 6px 6px;
    border-radius: 99px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    animation: zoom-in 0.2s cubic-bezier(0.175, 0.885, 0.32, 1.25) both;
  }

  @keyframes zoom-in {
    from { opacity: 0; transform: scale(0.9) translateY(4px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  .target-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 99px;
    padding: 3px 10px;
    font-size: 9.5px;
    font-weight: 750;
    text-transform: uppercase;
    letter-spacing: 0.08em;
    font-family: 'Outfit', 'Inter', sans-serif;
    animation: slide-in 0.22s cubic-bezier(0.175, 0.885, 0.32, 1.2) both;
    flex-shrink: 0;
  }

  .target-icon {
    font-size: 11px;
    line-height: 1;
  }

  .target-text {
    max-width: 120px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes slide-in {
    from { opacity: 0; transform: translateX(8px); }
    to   { opacity: 1; transform: translateX(0); }
  }

  .pulse-container {
    position: relative;
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .ring {
    position: absolute;
    border-radius: 50%;
    border: 1.5px solid var(--color-accent-tangerine, #ff6b35);
    opacity: 0;
  }

  .active .ring {
    animation: expand 1.8s cubic-bezier(0.1, 0.8, 0.1, 1) infinite;
  }

  .initializing .ring {
    border-color: #f59e0b;
    animation: expand 2.8s cubic-bezier(0.2, 0.8, 0.2, 1) infinite;
  }

  .ring1 { width: 100%; height: 100%; }
  .ring2 { width: 100%; height: 100%; animation-delay: 0.9s !important; }
  .initializing .ring2 { animation-delay: 1.4s !important; }

  .core {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: var(--color-accent-tangerine, #ff6b35);
    box-shadow: 0 0 8px var(--color-accent-tangerine, #ff6b35);
    transition: all 0.25s cubic-bezier(0.175, 0.885, 0.32, 1.2);
  }

  .initializing .core {
    background: #f59e0b;
    box-shadow: 0 0 8px #f59e0b;
  }

  .processing .ring {
    border-color: var(--color-accent-blue, #38bdf8);
    animation: expand 1.8s cubic-bezier(0.1, 0.8, 0.1, 1) infinite;
  }

  .processing .ring2 {
    animation-delay: 0.9s !important;
  }

  .processing .core {
    background: var(--color-accent-blue, #38bdf8);
    box-shadow: 0 0 10px var(--color-accent-blue, #38bdf8);
  }

  @keyframes expand {
    0%   { opacity: 0.8; transform: scale(0.3); }
    100% { opacity: 0;   transform: scale(1.6); }
  }
</style>
