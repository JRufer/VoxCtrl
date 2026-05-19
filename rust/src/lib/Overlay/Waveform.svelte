<script lang="ts">
  import { onDestroy } from "svelte";

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

<div class="waveform-card">
  <div class="bars">
    {#each heights as h, i}
      <div
        class="bar"
        style="height: {h}px; opacity: {recording ? 0.7 + (h / 60) * 0.3 : 0.3}"
      ></div>
    {/each}
  </div>
</div>

<style>
  .waveform-card {
    background: rgba(15, 15, 25, 0.88);
    backdrop-filter: blur(12px);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 16px;
    padding: 16px 20px;
    display: flex;
    align-items: center;
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
</style>
