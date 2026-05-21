<script lang="ts">
  import { status } from "../../stores/status";
  let { recording = false } = $props();
</script>

<div class="pulse-outer">
  <div class="pulse-row">
    <div class="pulse-container" class:active={recording}>
      <div class="ring ring1"></div>
      <div class="ring ring2"></div>
      <div class="core"></div>
    </div>
    {#if recording}
      <span class="target-badge">
        <span class="target-icon">🎯</span>
        <span class="target-text">{$status.active_target_label || "Focused Window"}</span>
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

  .ring1 { width: 100%; height: 100%; }
  .ring2 { width: 100%; height: 100%; animation-delay: 1s !important; }

  .core {
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: var(--recording-color, #e94560);
    transition: background 0.3s;
  }

  @keyframes expand {
    0%   { opacity: 0.8; transform: scale(0.3); }
    100% { opacity: 0;   transform: scale(1.5); }
  }
</style>
