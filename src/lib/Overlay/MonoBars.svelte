<script lang="ts">
  import { onMount } from "svelte";
  import { onLevelFrame } from "./audio-level";
  import { status } from "../../stores/status";
  import { liveTextShown } from "../../stores/liveTranscript";
  import LiveText from "./LiveText.svelte";

  let { recording = false, active = true } = $props();

  const BAR_COUNT = 5;
  const MONO_BAR_MIN = 8;
  const MONO_BAR_MAX = 52;

  let barHeights = $state<number[]>(Array(BAR_COUNT).fill(MONO_BAR_MIN));

  const isReady = $derived($status.audio_ready !== false);
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  // Mirrors mono_bar_height() in src-tauri/src/overlay.rs
  function monoBarHeight(
    i: number,
    n: number,
    level: number,
    phase: number,
    rec: boolean,
    proc: boolean,
    ready: boolean,
    noise: number
  ): number {
    const mid = (n - 1) / 2;
    const envelope = 1.0 - (Math.abs(i - mid) / (mid + 1)) * 0.4;
    let amp: number;
    if (proc) {
      amp = (Math.sin(phase * 4.0 - i * 0.85) * 0.5 + 0.5) * envelope;
    } else if (rec && !ready) {
      amp = 0;
    } else if (rec) {
      const ripple = Math.sin(phase * 3.0 - i * 0.8);
      amp = Math.min(1, Math.max(0, level * envelope * (0.9 + 0.1 * ripple) * (0.85 + 0.3 * noise)));
    } else {
      amp = 0;
    }
    return MONO_BAR_MIN + (MONO_BAR_MAX - MONO_BAR_MIN) * amp;
  }

  onMount(() => {
    let phase = 0;

    return onLevelFrame((currentVolume) => {
      phase += 0.016;

      const ready = $status.audio_ready !== false;
      const heights = new Array(BAR_COUNT);
      for (let i = 0; i < BAR_COUNT; i++) {
        heights[i] = monoBarHeight(i, BAR_COUNT, currentVolume, phase, recording, $status.processing, ready, Math.random());
      }
      barHeights = heights;
    });
  });
</script>

<div class="mono" class:on={active} class:live={$liveTextShown}>
  <div class="status">
    <span
      class="dot"
      class:lit={!$status.processing && !(recording && !isReady)}
      class:processing={$status.processing}
      class:standby={recording && !isReady}
    ></span>
    <span class="status-text">
      {#if $status.processing}
        PROCESSING
      {:else if recording && !isReady}
        STANDBY
      {:else}
        LISTENING
      {/if}
    </span>
  </div>

  <div class="bars">
    {#each barHeights as h}
      <div
        class="bar"
        class:dim={recording && !isReady}
        class:processing={$status.processing}
        style="height: {h}px"
      ></div>
    {/each}
  </div>

  <div class="baseline"></div>
  <div class="target">{targetLabel}</div>

  {#if $liveTextShown}
    <div class="caption-rule"></div>
    <LiveText
      class="caption"
      lines={3}
      placeholder="…"
    />
  {/if}
</div>

<style>
  /* Hyper-minimal black & white panel: fades in/out, no motion or color */
  .mono {
    width: 190px;
    height: 112px;
    background: #000000;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-radius: 6px;
    position: relative;
    font-family: 'Outfit', 'Inter', sans-serif;
    pointer-events: none;
    user-select: none;
    opacity: 0;
    transition: opacity 0.28s ease;
  }

  .mono.on {
    opacity: 1;
  }

  /* Live transcript: bars and target lift; a caption sits below a hairline */
  .mono {
    --lift: 0px;
  }

  .mono.live {
    --lift: 52px;
    height: 164px;
  }

  .mono .bars,
  .mono .baseline {
    bottom: calc(28px + var(--lift));
  }

  .mono .target {
    bottom: calc(6px + var(--lift));
  }

  .caption-rule {
    position: absolute;
    left: 18px;
    width: 154px;
    bottom: 50px;
    height: 1px;
    background: rgba(255, 255, 255, 0.12);
  }

  .mono :global(.caption) {
    position: absolute;
    left: 18px;
    width: 154px;
    bottom: 10px;
    text-align: center;
    --lt-size: 9px;
    --lt-line-height: 12px;
    --lt-weight: 500;
    --lt-tracking: 0.03em;
    --lt-color: #f5f5f5;
    --lt-tentative: rgba(255, 255, 255, 0.4);
    --lt-placeholder: rgba(255, 255, 255, 0.3);
    --lt-flash: #ffffff;
  }

  .mono :global(.caption .lt-placeholder) {
    font-style: normal;
    letter-spacing: 0.3em;
  }

  .status {
    position: absolute;
    left: 18px;
    top: 14px;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .dot {
    width: 6px;
    height: 6px;
    border-radius: 3px;
    border: 1px solid rgba(245, 245, 245, 0.7);
    background: transparent;
  }

  .dot.lit {
    background: #f5f5f5;
    animation: mono-blink 1.2s ease-in-out infinite;
  }

  .dot.processing {
    animation: mono-blink-dim 1.2s ease-in-out infinite;
  }

  .dot.standby {
    opacity: 0.35;
  }

  @keyframes mono-blink {
    0%, 100% { opacity: 0.5; }
    50% { opacity: 1; }
  }

  @keyframes mono-blink-dim {
    0%, 100% { opacity: 0.3; }
    50% { opacity: 1; }
  }

  .status-text {
    color: rgba(255, 255, 255, 0.55);
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0.25em;
  }

  .bars {
    position: absolute;
    left: 18px;
    bottom: 28px;
    width: 154px;
    height: 52px;
    display: flex;
    align-items: flex-end;
    justify-content: center;
    gap: 14px;
  }

  .bar {
    width: 12px;
    background: #f5f5f5;
    border-radius: 6px;
    opacity: 1;
    transition: height 0.05s linear, opacity 0.2s ease;
  }

  .bar.dim {
    opacity: 0.3;
  }

  .bar.processing {
    opacity: 0.55;
  }

  .baseline {
    position: absolute;
    left: 18px;
    bottom: 28px;
    width: 154px;
    height: 1px;
    background: rgba(255, 255, 255, 0.12);
  }

  .target {
    position: absolute;
    left: 18px;
    bottom: 6px;
    width: 154px;
    text-align: center;
    color: rgba(255, 255, 255, 0.4);
    font-size: 8.5px;
    font-weight: 600;
    letter-spacing: 0.03em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
