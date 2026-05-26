<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, speaking = false } = $props();

  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  // SVG elements reference
  let path1El: SVGPathElement | null = null;
  let path2El: SVGPathElement | null = null;
  let path3El: SVGPathElement | null = null;

  onMount(() => {
    // Listen to real-time audio levels emitted from the Rust backend
    listen<number>("audio-level", (event) => {
      // Calibrated audio signal multiplier
      targetVolume = Math.min(1.0, event.payload * 100.0);
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let time = 0;
    const width = 288;
    const height = 55;

    function getWavePath(amplitude: number, frequency: number, phase: number, yOffset: number) {
      let points = [];
      for (let x = 0; x <= width; x += 4) {
        const y = yOffset + Math.sin(x * frequency + phase) * amplitude;
        points.push(`${x},${y}`);
      }
      return `M 0,${height} L 0,${yOffset} ` + points.map(p => `L ${p}`).join(' ') + ` L ${width},${height} Z`;
    }

    function updateAnimation() {
      time += 1;
      
      // Snappy volume interpolation
      currentVolume += (targetVolume - currentVolume) * 0.38;
      targetVolume *= 0.84;

      // Layer 1 (Back Wave - Deep Blue)
      const phase1 = time * 0.035;
      const amp1 = ($status.processing ? (5.0 + Math.sin(time * 0.02) * 2.5) : 2.5) + currentVolume * 14.0;
      const yOff1 = (38 - ($status.processing ? (4.0 + Math.sin(time * 0.015) * 2.0) : 0)) - currentVolume * 12.0; // Sea level rises on activity
      const path1 = getWavePath(amp1, 0.016, phase1, yOff1);

      // Layer 2 (Middle Wave - Cyan Aqua)
      const phase2 = -time * 0.05; // Moves in reverse
      const amp2 = ($status.processing ? (6.0 + Math.cos(time * 0.025) * 3.0) : 2.0) + currentVolume * 18.0;
      const yOff2 = (34 - ($status.processing ? (5.0 + Math.cos(time * 0.02) * 2.5) : 0)) - currentVolume * 14.0;
      const path2 = getWavePath(amp2, 0.024, phase2, yOff2);

      // Layer 3 (Front Wave - Bright Ice Teal)
      const phase3 = time * 0.065;
      const amp3 = ($status.processing ? (7.0 + Math.sin(time * 0.03) * 3.5) : 1.5) + currentVolume * 22.0;
      const yOff3 = (28 - ($status.processing ? (6.0 + Math.sin(time * 0.025) * 3.0) : 0)) - currentVolume * 16.0;
      const path3 = getWavePath(amp3, 0.020, phase3, yOff3);

      if (path1El) path1El.setAttribute("d", path1);
      if (path2El) path2El.setAttribute("d", path2);
      if (path3El) path3El.setAttribute("d", path3);

      animationFrameId = requestAnimationFrame(updateAnimation);
    }

    animationFrameId = requestAnimationFrame(updateAnimation);

    return () => {
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });
</script>

<div class="voice-card-container" class:processing={$status.processing}>
  <div class="header-row">
    <span class="activity-label">{$status.processing ? "AI Thinking" : "Voice Activity"}</span>
    <span class="target-badge" class:processing={$status.processing}>
      <span class="target-text">{$status.processing ? "Processing..." : ($status.active_target_label || "Focused Window")}</span>
    </span>
  </div>
  
  <div class="ocean-container">
    <svg width="288" height="55" viewBox="0 0 288 55" xmlns="http://www.w3.org/2000/svg">
      <!-- Wave 1 (Deep Ocean Blue) -->
      <path 
        bind:this={path1El} 
        fill="rgba(2, 132, 199, 0.35)" 
        stroke="rgba(2, 132, 199, 0.15)"
        stroke-width="1"
      />
      <!-- Wave 2 (Rich Cyan Aqua) -->
      <path 
        bind:this={path2El} 
        fill="rgba(6, 182, 212, 0.5)" 
        stroke="rgba(6, 182, 212, 0.2)"
        stroke-width="1"
      />
      <!-- Wave 3 (Vibrant Ice Teal) -->
      <path 
        bind:this={path3El} 
        fill="rgba(34, 211, 238, 0.65)" 
        stroke="rgba(34, 211, 238, 0.4)"
        stroke-width="1.5"
      />
    </svg>
  </div>
</div>

<style>
  .voice-card-container {
    width: 320px;
    height: 120px;
    background: rgba(18, 18, 22, 0.94);
    border: 1.2px solid rgba(255, 255, 255, 0.05);
    border-radius: 28px;
    padding: 14px 16px;
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6), inset 0 1px 0 rgba(255, 255, 255, 0.05);
    backdrop-filter: blur(16px);
    animation: slideUpIn 0.25s cubic-bezier(0.16, 1, 0.3, 1) both;
    user-select: none;
    pointer-events: none;
    transition: border-color 0.3s ease, box-shadow 0.3s ease;
  }

  .voice-card-container.processing {
    border-color: rgba(6, 182, 212, 0.35);
    box-shadow: 0 16px 40px rgba(0, 0, 0, 0.6), 0 0 15px rgba(6, 182, 212, 0.15), inset 0 1px 0 rgba(255, 255, 255, 0.05);
  }

  @keyframes slideUpIn {
    from {
      opacity: 0;
      transform: translateY(12px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  .header-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
  }

  .activity-label {
    font-family: 'Inter', sans-serif;
    font-size: 11px;
    font-weight: 550;
    color: rgba(255, 255, 255, 0.35);
    letter-spacing: -0.01em;
  }

  .target-badge {
    display: inline-flex;
    align-items: center;
    border: 1px solid rgba(16, 185, 129, 0.4);
    background: rgba(16, 185, 129, 0.07);
    color: #10b981;
    font-family: 'Inter', sans-serif;
    font-size: 10.5px;
    font-weight: 600;
    border-radius: 6px;
    padding: 2.5px 9px;
    max-width: 130px;
    transition: all 0.3s ease;
  }

  .target-badge.processing {
    border-color: rgba(6, 182, 212, 0.4);
    background: rgba(6, 182, 212, 0.07);
    color: #06b6d4;
    animation: badgePulse 1.8s ease-in-out infinite;
  }

  @keyframes badgePulse {
    0%, 100% { opacity: 0.9; }
    50% { opacity: 0.6; }
  }

  .target-text {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ocean-container {
    display: flex;
    align-items: flex-end;
    justify-content: center;
    height: 55px;
    width: 100%;
    margin-top: auto;
    overflow: hidden;
    border-radius: 0 0 16px 16px;
  }

  svg {
    display: block;
    overflow: hidden;
  }
</style>
