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

  $effect(() => {
    if (recording && $status.audio_ready !== false) {
      timer = setInterval(() => {
        heights = Array.from({ length: BAR_COUNT }, (_, i) => {
          const env = envelope(i);
          const base = 2 + env * 8;
          const noise = Math.random() * env * 56;
          return base + noise;
        });
      }, 60);
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
      <span class="mic-dot" class:initializing={recording && !isReady}></span>
      <span class="info-text">{recording && !isReady ? "CONNECTING..." : "MIC"}</span>
      <span class="sep">·</span>
      <span class="info-text">{recording && !isReady ? "preparing stream" : "voice overlay"}</span>
    </div>
    {#if recording}
      <div class="target-pill" style="animation: pill-in 0.25s ease both;">
        <span class="arrow">→</span>
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
          class:active={recording && isReady}
          style="
            height: {h}px;
            --glow: {recording && isReady ? Math.round(env * 18) : 0}px;
            --opacity: {recording && isReady ? (0.45 + env * 0.55) : 0.18};
          "
        ></div>
      {/each}
    </div>
  </div>
</div>

<style>
  .wave-widget {
    width: 580px;
    background: linear-gradient(160deg, #04111f 0%, #071828 100%);
    border: 1px solid rgba(0, 180, 230, 0.18);
    border-radius: 18px;
    padding: 14px 20px 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    box-shadow:
      0 0 0 1px rgba(0, 200, 255, 0.06),
      0 8px 40px rgba(0, 0, 0, 0.7),
      inset 0 1px 0 rgba(255, 255, 255, 0.04);
    pointer-events: none;
    user-select: none;
  }

  /* ── Info bar ────────────────────────────────── */
  .info-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 18px;
  }

  .info-left {
    display: flex;
    align-items: center;
    gap: 7px;
    color: rgba(140, 200, 230, 0.65);
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .mic-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #00e5c0;
    box-shadow: 0 0 6px #00e5c0;
    flex-shrink: 0;
  }

  .mic-dot.initializing {
    background: #ff9100;
    box-shadow: 0 0 8px #ff9100;
    animation: pulse-orange 0.6s ease-in-out infinite alternate;
  }

  @keyframes pulse-orange {
    from { opacity: 0.4; transform: scale(0.95); }
    to { opacity: 1.0; transform: scale(1.25); }
  }

  .sep {
    opacity: 0.4;
  }

  /* Target pill — top-right */
  .target-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: transparent;
    border: 1.5px solid rgba(0, 195, 255, 0.55);
    border-radius: 20px;
    padding: 2px 11px 2px 9px;
    color: #4dd9f7;
    font-size: 10px;
    font-weight: 700;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    box-shadow: 0 0 10px rgba(0, 195, 255, 0.2);
  }

  .arrow {
    font-size: 11px;
    opacity: 0.8;
  }

  .pill-text {
    max-width: 140px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes pill-in {
    from { opacity: 0; transform: translateX(6px); }
    to   { opacity: 1; transform: translateX(0); }
  }

  /* ── Wave stage ──────────────────────────────── */
  .wave-stage {
    position: relative;
    height: 80px;
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
      rgba(0, 180, 220, 0.35) 0px,
      rgba(0, 180, 220, 0.35) 3px,
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
    gap: 2.5px;
    width: 100%;
    height: 100%;
    z-index: 1;
  }

  .bar {
    width: 3px;
    border-radius: 2px;
    background: #00c8f0;
    opacity: var(--opacity, 0.18);
    /* Symmetric bars — grow from centre */
    transition: height 0.06s cubic-bezier(0.4, 0, 0.2, 1),
                opacity 0.06s ease;
    min-height: 2px;
  }

  .bar.active {
    box-shadow:
      0 0 var(--glow) rgba(0, 200, 240, 0.9),
      0 0 calc(var(--glow) * 2) rgba(0, 200, 240, 0.35);
  }
</style>
