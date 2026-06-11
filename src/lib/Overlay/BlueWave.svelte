<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, speaking = false, active = true } = $props();

  const W = 380;
  const H = 90;

  let path1 = $state("");
  let path2 = $state("");
  let path3 = $state("");
  let buoyY = $state(60);

  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  const isReady = $derived($status.audio_ready !== false);
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  function wavePath(amp: number, freq: number, phase: number, yOff: number): string {
    let d = `M 0 ${H} L 0 ${yOff.toFixed(1)}`;
    for (let x = 0; x <= W; x += 8) {
      const y = yOff + Math.sin(x * freq + phase) * amp;
      d += ` L ${x} ${y.toFixed(1)}`;
    }
    return d + ` L ${W} ${H} Z`;
  }

  onMount(() => {
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let phase = 0;

    function update() {
      phase += 0.016;
      currentVolume += (targetVolume - currentVolume) * 0.35;
      targetVolume *= 0.86;

      const lift = currentVolume;
      const surge = $status.processing ? 4.0 + 2.5 * Math.sin(phase * 1.6) : 0;

      const y1 = 64 - lift * 20 - surge;
      const y2 = 56 - lift * 24 - surge * 1.2;
      const y3 = 47 - lift * 28 - surge * 1.4;

      const a1 = ($status.processing ? 5.0 : 2.5) + lift * 14;
      const a2 = ($status.processing ? 6.0 : 2.0) + lift * 18;
      const a3 = ($status.processing ? 7.0 : 1.5) + lift * 22;

      path1 = wavePath(a1, 0.014, phase * 2.1, y1);
      path2 = wavePath(a2, 0.022, -phase * 3.0, y2);
      path3 = wavePath(a3, 0.018, phase * 3.9, y3);

      // The buoy bobs on the front wave's surface (panel coordinates)
      buoyY = 38 + y3 - 30 + Math.sin(phase * 2.2) * 2.5;

      animationFrameId = requestAnimationFrame(update);
    }
    animationFrameId = requestAnimationFrame(update);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });
</script>

<div class="ocean" class:on={active} class:processing={$status.processing}>
  <div class="moon"></div>

  <span class="title">OCEAN WAVE</span>
  <span class="subtitle">
    {#if $status.processing}
      deep current — processing
    {:else if recording && !isReady}
      low tide — preparing
    {:else}
      high tide — listening
    {/if}
  </span>

  <!-- Water fills on load and drains away on unload -->
  <div class="water">
    <svg viewBox="0 0 {W} {H}" preserveAspectRatio="none">
      <path class="w1" d={path1} />
      <path class="w2" d={path2} />
      <path class="w3" d={path3} />
    </svg>
  </div>

  {#each [0, 1, 2] as i}
    <div class="bubble" style="left: {64 + i * 112}px; animation-delay: {i * 1.4}s;"></div>
  {/each}

  <!-- Floating buoy target tag -->
  <div class="buoy" style="top: {buoyY}px;">
    <span class="buoy-dot"></span>
    <span class="buoy-label">
      {#if $status.processing}
        Sounding the depths…
      {:else if recording && !isReady}
        Casting off…
      {:else}
        {targetLabel}
      {/if}
    </span>
  </div>
</div>

<style>
  .ocean {
    position: relative;
    width: 380px;
    height: 128px;
    border-radius: 24px;
    overflow: hidden;
    background: linear-gradient(180deg, rgba(8, 18, 32, 0.96) 0%, rgba(3, 9, 18, 0.97) 100%);
    border: 1.2px solid rgba(34, 211, 238, 0.25);
    box-shadow: 0 14px 40px rgba(0, 0, 0, 0.6), 0 0 24px rgba(8, 145, 178, 0.22);
    pointer-events: none;
    user-select: none;
    font-family: 'Outfit', 'Inter', sans-serif;
    opacity: 0;
    transform: translateY(16px);
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.2),
      opacity 0.3s ease;
  }

  .ocean.on {
    opacity: 1;
    transform: translateY(0);
  }

  .ocean.processing {
    border-color: rgba(56, 189, 248, 0.35);
  }

  .moon {
    position: absolute;
    top: 10px;
    right: 62px;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: rgba(224, 242, 254, 0.85);
    box-shadow: 0 0 16px rgba(186, 230, 253, 0.6);
    animation: moon-glow 2.4s ease-in-out infinite;
  }

  @keyframes moon-glow {
    0%, 100% { opacity: 0.55; }
    50% { opacity: 0.8; }
  }

  .title {
    position: absolute;
    left: 18px;
    top: 12px;
    color: rgba(186, 230, 253, 0.55);
    font-size: 9.5px;
    font-weight: 800;
    letter-spacing: 0.22em;
  }

  .subtitle {
    position: absolute;
    left: 18px;
    top: 25px;
    color: rgba(125, 211, 252, 0.35);
    font-size: 8px;
    font-weight: 500;
    letter-spacing: 0.08em;
  }

  /* Water layer slides up to fill the pool, drains down on unload */
  .water {
    position: absolute;
    left: 0;
    right: 0;
    top: 38px;
    height: 90px;
    transform: translateY(92px);
    transition: transform 0.45s cubic-bezier(0.22, 1, 0.36, 1);
  }

  .on .water {
    transform: translateY(0);
  }

  svg {
    display: block;
    width: 100%;
    height: 100%;
  }

  .w1 { fill: rgba(2, 132, 199, 0.35); stroke: rgba(2, 132, 199, 0.2); stroke-width: 1; }
  .w2 { fill: rgba(6, 182, 212, 0.45); stroke: rgba(6, 182, 212, 0.25); stroke-width: 1; }
  .w3 { fill: rgba(34, 211, 238, 0.6); stroke: rgba(165, 243, 252, 0.55); stroke-width: 1.5; }

  .processing .w1 { fill: rgba(30, 64, 175, 0.4); }
  .processing .w2 { fill: rgba(59, 130, 246, 0.45); }
  .processing .w3 { fill: rgba(96, 165, 250, 0.55); stroke: rgba(147, 197, 253, 0.6); }

  .bubble {
    position: absolute;
    bottom: -8px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    border: 1px solid rgba(165, 243, 252, 0.7);
    background: rgba(165, 243, 252, 0.12);
    animation: bubble-rise 4.2s linear infinite;
    opacity: 0;
  }

  @keyframes bubble-rise {
    0% { transform: translateY(0); opacity: 0; }
    12% { opacity: 0.55; }
    85% { opacity: 0.25; }
    100% { transform: translateY(-78px); opacity: 0; }
  }

  .buoy {
    position: absolute;
    left: 50%;
    transform: translateX(-50%);
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 12px 0 10px;
    border-radius: 12px;
    background: rgba(8, 47, 73, 0.92);
    border: 1px solid rgba(125, 211, 252, 0.5);
    opacity: 0;
    transition: opacity 0.3s ease 0.2s;
  }

  .on .buoy {
    opacity: 1;
  }

  .buoy-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: #22d3ee;
    animation: moon-glow 1.15s ease-in-out infinite;
    flex-shrink: 0;
  }

  .processing .buoy-dot {
    background: #60a5fa;
  }

  .buoy-label {
    color: #e0f2fe;
    font-size: 10px;
    font-weight: 700;
    max-width: 130px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
