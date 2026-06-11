<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, active = true } = $props();

  // Oscilloscope: a scrolling ring buffer of samples rendered as one line trace
  const POINTS = 126;
  const W = 500;
  const H = 78;

  let tracePath = $state("");
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  const isReady = $derived($status.audio_ready !== false);
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  onMount(() => {
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    const history = new Float32Array(POINTS);
    let phase = 0;

    function update() {
      phase += 0.016;
      currentVolume += (targetVolume - currentVolume) * 0.35;
      targetVolume *= 0.86;

      let sample = 0;
      if ($status.processing) {
        sample = 0.55 * Math.sin(phase * 9.0) * (0.6 + 0.4 * Math.sin(phase * 1.7));
      } else if (recording && $status.audio_ready === false) {
        sample = 0.05 * (Math.random() * 2 - 1);
      } else if (recording) {
        const noise = Math.random() * 2 - 1;
        sample = Math.min(1, currentVolume * 1.5) * (0.45 * Math.sin(phase * 24.0) + 0.55 * noise);
      }

      history.copyWithin(0, 1);
      history[POINTS - 1] = sample;

      let d = "";
      for (let i = 0; i < POINTS; i++) {
        const x = (i * W) / (POINTS - 1);
        const y = Math.min(H - 2, Math.max(2, H / 2 - history[i] * 35));
        d += i === 0 ? `M ${x.toFixed(0)} ${y.toFixed(1)}` : ` L ${x.toFixed(0)} ${y.toFixed(1)}`;
      }
      tracePath = d;

      animationFrameId = requestAnimationFrame(update);
    }
    animationFrameId = requestAnimationFrame(update);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });
</script>

<div
  class="scope"
  class:on={active}
  class:processing={$status.processing}
  class:initializing={recording && !isReady}
>
  <div class="scope-inner">
    <div class="header">
      <span class="led"></span>
      <span class="title">WAVEFORM // OSC-01</span>
      <span class="subtitle">
        {#if $status.processing}
          · TRANSCRIBING
        {:else if recording && !isReady}
          · CALIBRATING
        {:else}
          · LIVE TRACE
        {/if}
      </span>
      <span class="spacer"></span>
      <span class="target-chip">
        <span class="tgt">TGT ▸</span>
        <span class="tgt-label">{targetLabel}</span>
      </span>
    </div>

    <div class="stage">
      <div class="grid-h g1"></div>
      <div class="grid-h g2"></div>
      <div class="grid-h g3"></div>
      {#each [1, 2, 3, 4] as t}
        <div class="grid-v" style="left: {t * 20}%"></div>
      {/each}
      <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none">
        <path class="glow" d={tracePath} />
        <path class="trace" d={tracePath} />
      </svg>
    </div>
  </div>
</div>

<style>
  /* CRT panel: powers on by expanding from a scanline, collapses back off */
  .scope {
    width: 540px;
    height: 124px;
    background: linear-gradient(180deg, #07140c 0%, #030a07 100%);
    border: 1px solid rgba(74, 222, 128, 0.25);
    border-radius: 10px;
    box-shadow: 0 10px 36px rgba(0, 0, 0, 0.6), 0 0 18px rgba(34, 197, 94, 0.14);
    overflow: hidden;
    pointer-events: none;
    user-select: none;
    transform: scaleY(0.04);
    opacity: 0;
    transition:
      transform 0.32s cubic-bezier(0.175, 0.885, 0.32, 1.2),
      opacity 0.28s ease;
  }

  .scope.on {
    transform: scaleY(1);
    opacity: 1;
  }

  .scope-inner {
    width: 100%;
    height: 100%;
    padding: 10px 20px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .header {
    display: flex;
    align-items: center;
    gap: 8px;
    height: 18px;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .led {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #4ade80;
    box-shadow: 0 0 7px #4ade80;
    animation: led-blink 1.1s ease-in-out infinite;
    flex-shrink: 0;
  }

  .initializing .led {
    background: #f59e0b;
    box-shadow: 0 0 7px #f59e0b;
  }

  .processing .led {
    background: #38bdf8;
    box-shadow: 0 0 7px #38bdf8;
  }

  @keyframes led-blink {
    0%, 100% { opacity: 0.4; }
    50% { opacity: 1; }
  }

  .title {
    color: #bbf7d0;
    font-size: 10px;
    font-weight: 800;
    letter-spacing: 0.12em;
  }

  .subtitle {
    color: rgba(187, 247, 208, 0.35);
    font-size: 9px;
    font-weight: 500;
    letter-spacing: 0.1em;
  }

  .spacer { flex: 1; }

  .target-chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    background: rgba(74, 222, 128, 0.06);
    border: 1px solid rgba(74, 222, 128, 0.3);
    border-radius: 4px;
    padding: 2px 8px;
  }

  .tgt {
    color: #4ade80;
    font-size: 8.5px;
    font-weight: 800;
    letter-spacing: 0.1em;
  }

  .tgt-label {
    color: #d1fae5;
    font-size: 8.5px;
    font-weight: 700;
    letter-spacing: 0.06em;
    max-width: 150px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .stage {
    position: relative;
    flex: 1;
  }

  .grid-h {
    position: absolute;
    left: 0;
    right: 0;
    height: 1px;
    background: rgba(74, 222, 128, 0.07);
  }
  .g1 { top: 25%; }
  .g2 { top: 50%; background: rgba(74, 222, 128, 0.14); }
  .g3 { top: 75%; }

  .grid-v {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: rgba(74, 222, 128, 0.05);
  }

  svg {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }

  .trace {
    fill: none;
    stroke: #a7f3d0;
    stroke-width: 1.8;
    vector-effect: non-scaling-stroke;
  }

  .glow {
    fill: none;
    stroke: #4ade80;
    stroke-width: 6;
    opacity: 0.16;
    vector-effect: non-scaling-stroke;
  }

  .initializing .trace { stroke: #fcd34d; }
  .initializing .glow { stroke: #f59e0b; }
  .processing .trace { stroke: #7dd3fc; }
  .processing .glow { stroke: #38bdf8; }
</style>
