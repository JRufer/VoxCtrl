<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, speaking = false, active = true } = $props();

  // VU-meter LED dot matrix: 20 columns x 6 rows, lit bottom-up
  const COLS = 20;
  const ROWS = 6;

  let colLevels = $state<number[]>(Array(COLS).fill(0));

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

    let phase = 0;
    const levels = new Float32Array(COLS);

    function update() {
      phase += 0.016;
      currentVolume += (targetVolume - currentVolume) * 0.35;
      targetVolume *= 0.86;

      for (let i = 0; i < COLS; i++) {
        let target: number;
        if ($status.processing) {
          target = 0.25 + 0.75 * Math.max(0, Math.sin(phase * 4.0 - i * 0.45));
        } else if (recording && $status.audio_ready === false) {
          target = 0.08 + 0.06 * Math.random();
        } else {
          const mid = (COLS - 1) / 2;
          const env = Math.exp(-((i - mid) ** 2) / 42);
          target = Math.min(1, currentVolume * (0.55 + 0.65 * Math.random()) * env * 1.9);
        }
        // Fast attack, slow decay — like a real VU meter
        levels[i] = target > levels[i] ? target : levels[i] * 0.86;
      }
      colLevels = Array.from(levels);

      animationFrameId = requestAnimationFrame(update);
    }
    animationFrameId = requestAnimationFrame(update);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });

  function dotClass(row: number): string {
    // row 0 is the top dot: red, then amber, then green
    if (row === 0) return "red";
    if (row <= 2) return "amber";
    return "green";
  }
</script>

<div class="card-scene" class:on={active}>
  <div class="card" class:processing={$status.processing} class:initializing={recording && !isReady}>
    <div class="sheen"></div>

    <div class="chip">
      <span class="chip-line h1"></span>
      <span class="chip-line h2"></span>
      <span class="chip-line v"></span>
    </div>

    <span class="brand">VOXCTRL</span>
    <span class="brand-sub">VOICE CARD</span>

    <span class="stamp">
      <span class="stamp-dot"></span>
      {#if $status.processing}
        PROC
      {:else if recording && !isReady}
        INIT
      {:else}
        REC
      {/if}
    </span>

    <div class="matrix">
      {#each colLevels as lvl}
        <div class="col">
          {#each Array(ROWS) as _, r}
            <span
              class="dot {dotClass(r)}"
              class:lit={ROWS - r <= lvl * ROWS}
            ></span>
          {/each}
        </div>
      {/each}
    </div>

    <span class="field-label">TARGET</span>
    <span class="field-value">
      {#if $status.processing}
        Reading the card…
      {:else}
        {targetLabel}
      {/if}
    </span>
    <span class="card-number">•••• VOX</span>
  </div>
</div>

<style>
  /* Card deals in with a 3D flip, flips back out on unload */
  .card-scene {
    perspective: 900px;
    pointer-events: none;
    user-select: none;
  }

  .card {
    position: relative;
    width: 340px;
    height: 152px;
    border-radius: 16px;
    overflow: hidden;
    background: linear-gradient(135deg, #181b24 0%, #0b0d12 55%, #141925 100%);
    border: 1px solid rgba(255, 255, 255, 0.12);
    box-shadow: 0 18px 44px rgba(0, 0, 0, 0.6);
    font-family: 'Outfit', 'Inter', sans-serif;
    transform: rotateY(88deg);
    opacity: 0;
    transition:
      transform 0.38s cubic-bezier(0.175, 0.885, 0.32, 1.2),
      opacity 0.3s ease,
      border-color 0.3s ease;
  }

  .on .card {
    transform: rotateY(0deg);
    opacity: 1;
  }

  .card.processing {
    border-color: rgba(56, 189, 248, 0.4);
  }

  /* Holographic sheen drifting across the face */
  .sheen {
    position: absolute;
    top: -20px;
    bottom: -20px;
    width: 64px;
    background: linear-gradient(90deg, transparent 0%, rgba(255, 255, 255, 0.06) 50%, transparent 100%);
    animation: sheen-drift 6.5s linear infinite;
  }

  @keyframes sheen-drift {
    from { left: -70px; }
    to { left: 410px; }
  }

  .chip {
    position: absolute;
    left: 20px;
    top: 18px;
    width: 38px;
    height: 28px;
    border-radius: 6px;
    background: linear-gradient(160deg, #f6d27a 0%, #d9a946 55%, #b9842b 100%);
  }

  .chip-line {
    position: absolute;
    background: rgba(70, 45, 0, 0.45);
  }
  .chip-line.h1 { left: 0; right: 0; top: 9px; height: 1.5px; }
  .chip-line.h2 { left: 0; right: 0; top: 18px; height: 1.5px; }
  .chip-line.v { top: 0; bottom: 0; left: 18px; width: 1.5px; }

  .brand {
    position: absolute;
    left: 68px;
    top: 16px;
    color: #f3f4f6;
    font-size: 14px;
    font-weight: 850;
    letter-spacing: 0.08em;
  }

  .brand-sub {
    position: absolute;
    left: 68px;
    top: 34px;
    color: rgba(255, 255, 255, 0.38);
    font-size: 7.5px;
    font-weight: 800;
    letter-spacing: 0.4em;
  }

  .stamp {
    position: absolute;
    right: 20px;
    top: 18px;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 9px;
    border-radius: 5px;
    border: 1px solid rgba(244, 63, 94, 0.5);
    background: rgba(244, 63, 94, 0.08);
    color: #f43f5e;
    font-size: 8.5px;
    font-weight: 850;
    letter-spacing: 0.18em;
  }

  .initializing .stamp {
    border-color: rgba(245, 158, 11, 0.5);
    background: rgba(245, 158, 11, 0.08);
    color: #f59e0b;
  }

  .processing .stamp {
    border-color: rgba(56, 189, 248, 0.5);
    background: rgba(56, 189, 248, 0.08);
    color: #38bdf8;
  }

  .stamp-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    animation: stamp-blink 1.15s ease-in-out infinite;
  }

  @keyframes stamp-blink {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }

  .matrix {
    position: absolute;
    left: 22px;
    top: 56px;
    width: 296px;
    height: 51px;
    display: flex;
    gap: 4px;
  }

  .col {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .dot {
    width: 11px;
    height: 6px;
    border-radius: 2px;
    opacity: 0.1;
    transition: opacity 0.05s linear;
  }

  .dot.lit { opacity: 0.95; }
  .dot.red { background: #f43f5e; }
  .dot.amber { background: #fbbf24; }
  .dot.green { background: #34d399; }

  .processing .dot { background: #22d3ee; }

  .field-label {
    position: absolute;
    left: 22px;
    top: 118px;
    color: rgba(255, 255, 255, 0.3);
    font-size: 7px;
    font-weight: 800;
    letter-spacing: 0.36em;
  }

  .field-value {
    position: absolute;
    left: 22px;
    top: 128px;
    max-width: 200px;
    color: #e5e7eb;
    font-size: 11.5px;
    font-weight: 750;
    letter-spacing: 0.04em;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-number {
    position: absolute;
    right: 22px;
    top: 128px;
    color: rgba(255, 255, 255, 0.22);
    font-size: 11px;
    font-weight: 700;
    letter-spacing: 0.18em;
  }
</style>
