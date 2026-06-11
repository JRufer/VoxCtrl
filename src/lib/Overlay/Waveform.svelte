<script lang="ts">
  import { status } from "../../stores/status";

  let { recording = false } = $props();

  // 48 bars — center-weighted amplitude envelope
  const BAR_COUNT = 48;

  // Gaussian envelope so bars are tallest in the centre and taper at the edges
  function envelope(i: number): number {
    const mid = (BAR_COUNT - 1) / 2;
    const sigma = BAR_COUNT / 5;
    return Math.exp(-((i - mid) ** 2) / (2 * sigma ** 2));
  }

  let heights = $state(Array.from({ length: BAR_COUNT }, () => 2));
  let timer: ReturnType<typeof setInterval> | undefined;
  let offset = 0;

  $effect(() => {
    if (recording) {
      clearInterval(timer);
      timer = setInterval(() => {
        heights = Array.from({ length: BAR_COUNT }, (_, i) => {
          const env = envelope(i);
          const base = 2 + env * 8;
          const noise = Math.random() * env * 56;
          return base + noise;
        });
      }, 60);
    } else if ($status.processing) {
      clearInterval(timer);
      timer = setInterval(() => {
        offset += 0.22;
        heights = Array.from({ length: BAR_COUNT }, (_, i) => {
          const env = envelope(i);
          const base = 2 + env * 4;
          const wave = Math.sin(i * 0.45 - offset) * env * 28;
          return base + Math.abs(wave);
        });
      }, 30);
    } else {
      clearInterval(timer);
      heights = Array.from({ length: BAR_COUNT }, () => 2);
    }
    return () => clearInterval(timer);
  });

  const targetLabel = $derived($status.active_target_label || "Focused Window");
  const isReady = $derived($status.audio_ready !== false);
</script>

<div class="wave-widget">
  <!-- Top info bar -->
  <div class="info-bar">
    <div class="info-left">
      <span class="mic-dot" class:initializing={recording && !isReady} class:processing={$status.processing}></span>
      <span class="info-text">
        {#if $status.processing}
          PROCESSING...
        {:else}
          🎙️ MIC
        {/if}
      </span>
      <span class="sep">·</span>
      <span class="info-subtext">
        {#if $status.processing}
          thinking with AI
        {:else if recording && !isReady}
          preparing stream
        {:else}
          voice overlay
        {/if}
      </span>
    </div>
    {#if recording || $status.processing}
      <div class="target-pill">
        <span class="arrow">🎯</span>
        <span class="pill-text">{targetLabel}</span>
      </div>
    {/if}
  </div>

  <!-- Waveform area -->
  <div class="wave-stage">
    <!-- Dotted centre axis -->
    <div class="axis"></div>

    <!-- Bars -->
    <div class="bars">
      {#each heights as h, i}
        {@const env = envelope(i)}
        <div
          class="bar"
          class:active={(recording && isReady) || $status.processing}
          class:processing={$status.processing}
          style="
            height: {h}px;
            --glow: {((recording && isReady) || $status.processing) ? Math.round(env * 16) : 0}px;
            --opacity: {((recording && isReady) || $status.processing) ? (0.45 + env * 0.55) : 0.18};
          "
        ></div>
      {/each}
    </div>
  </div>
</div>

<style>
  .wave-widget {
    width: 540px;
    background: linear-gradient(160deg, #0d0e12 0%, #08090b 100%);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 12px; /* Option C */
    padding: 14px 20px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    box-shadow:
      0 10px 40px rgba(0, 0, 0, 0.65),
      inset 0 1px 0 rgba(255, 255, 255, 0.03);
    pointer-events: none;
    user-select: none;
    animation: zoom-in 0.2s cubic-bezier(0.175, 0.885, 0.32, 1.25) both;
  }

  @keyframes zoom-in {
    from { opacity: 0; transform: scale(0.95) translateY(6px); }
    to { opacity: 1; transform: scale(1) translateY(0); }
  }

  /* ── Info bar ────────────────────────────────── */
  .info-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 20px;
  }

  .info-left {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--color-obsidian-300);
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .mic-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-accent-tangerine);
    box-shadow: 0 0 8px var(--color-accent-tangerine);
    flex-shrink: 0;
  }

  .mic-dot.initializing {
    background: #f59e0b;
    box-shadow: 0 0 8px #f59e0b;
    animation: pulse-orange 0.6s ease-in-out infinite alternate;
  }

  .mic-dot.processing {
    background: var(--color-accent-blue);
    box-shadow: 0 0 8px var(--color-accent-blue);
    animation: pulse-cyan 0.6s ease-in-out infinite alternate;
  }

  @keyframes pulse-orange {
    from { opacity: 0.4; transform: scale(0.9); }
    to { opacity: 1.0; transform: scale(1.3); }
  }

  @keyframes pulse-cyan {
    from { opacity: 0.4; transform: scale(0.9); }
    to { opacity: 1.0; transform: scale(1.3); }
  }

  .info-subtext {
    font-weight: 500;
    color: rgba(255, 255, 255, 0.3);
  }

  .sep {
    color: rgba(255, 255, 255, 0.15);
  }

  /* Target pill — top-right */
  .target-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(255, 255, 255, 0.04);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 99px;
    padding: 2.5px 12px;
    color: #fff;
    font-size: 9px;
    font-weight: 750;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .arrow {
    font-size: 10px;
  }

  .pill-text {
    max-width: 140px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--color-obsidian-300);
  }

  /* ── Wave stage ──────────────────────────────── */
  .wave-stage {
    position: relative;
    height: 70px;
    display: flex;
    align-items: center;
  }

  /* Dotted horizontal axis line */
  .axis {
    position: absolute;
    left: 0;
    right: 0;
    top: 50%;
    height: 1px;
    background: repeating-linear-gradient(
      to right,
      rgba(255, 255, 255, 0.06) 0px,
      rgba(255, 255, 255, 0.06) 3px,
      transparent 3px,
      transparent 8px
    );
    pointer-events: none;
  }

  /* Bars */
  .bars {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    width: 100%;
    height: 100%;
    z-index: 1;
  }

  .bar {
    width: 3px;
    border-radius: 99px;
    background: var(--color-accent-tangerine);
    opacity: var(--opacity, 0.18);
    transition: height 0.06s cubic-bezier(0.175, 0.885, 0.32, 1.2),
                opacity 0.06s ease;
    min-height: 2px;
  }

  .bar.active {
    box-shadow:
      0 0 var(--glow) var(--color-accent-tangerine),
      0 0 calc(var(--glow) * 1.5) rgba(255, 107, 53, 0.3);
  }

  .bar.processing {
    background: var(--color-accent-blue);
    box-shadow:
      0 0 var(--glow) var(--color-accent-blue),
      0 0 calc(var(--glow) * 1.5) rgba(56, 189, 248, 0.3);
  }
</style>
