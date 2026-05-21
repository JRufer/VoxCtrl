<script lang="ts">
  import { onDestroy } from "svelte";
  import { status } from "../../stores/status";

  let { recording = false } = $props();

  let bars = 20;
  let heights = $state(Array(bars).fill(4));
  let timer: number;

  $effect(() => {
    if (recording) {
      timer = setInterval(() => {
        heights = heights.map(() =>
          recording ? 4 + Math.random() * 40 : 4
        );
      }, 80) as unknown as number;
    } else {
      clearInterval(timer);
      heights = Array(bars).fill(4);
    }
    return () => clearInterval(timer);
  });
</script>

<div class="waveform-container">
  <div class="waveform-card">
    <div class="bars">
      {#each heights as h}
        <div
          class="bar"
          style="height: {h}px; opacity: {recording ? 0.7 + (h / 60) * 0.3 : 0.3}"
        ></div>
      {/each}
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
  .waveform-container {
    display: flex;
    flex-direction: column;
    align-items: center;
  }

  .waveform-card {
    background: rgba(15, 15, 25, 0.88);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 16px;
    padding: 16px 20px;
    display: flex;
    align-items: center;
    gap: 12px;
    box-shadow: 0 4px 24px rgba(0, 0, 0, 0.5);
  }

  .bars {
    display: flex;
    align-items: center;
    gap: 3px;
    height: 48px;
  }

  .bar {
    width: 3px;
    background: var(--recording-color, #e94560);
    border-radius: 2px;
    transition: height 0.08s ease, opacity 0.08s ease;
    min-height: 4px;
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
