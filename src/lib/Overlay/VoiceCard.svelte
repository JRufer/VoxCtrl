<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, speaking = false } = $props();

  let barHeights = $state<number[]>(Array(45).fill(4));
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;
  let targetVolume = 0;
  let currentVolume = 0;

  onMount(() => {
    // Listen to real-time audio levels emitted from the Rust backend
    listen<number>("audio-level", (event) => {
      // Calibrated audio signal multiplier reduced by 1/3rd (100.0)
      targetVolume = Math.min(1.0, event.payload * 100.0);
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let time = 0;
    function updateAnimation() {
      time += 1;
      
      // Accelerate interpolation for immediate and snappy visual reaction ("fast attack")
      currentVolume += (targetVolume - currentVolume) * 0.42;
      
      // Accelerate decay rate for immediate visual drop-offs on silence ("fast release")
      targetVolume *= 0.82;

      const numBars = 45;
      const centerIdx = 22; // Center index of the symmetric array
      const rawHeights: number[] = [];

      for (let i = 0; i < numBars; i++) {
        // Gaussian bell curve centering larger base heights in the middle
        const centerDist = Math.abs(i - centerIdx);
        const baseHeight = Math.max(3, 15 * Math.exp(-(centerDist * centerDist) / 80));
        
        let targetHeight = baseHeight;
        if (currentVolume > 0.01) {
          // Propagate waves outwards by combining time and bar indices
          let wave = Math.sin(time * 0.18 + i * 0.35) * 0.45 + 0.55;
          // Add some organic high-frequency jitter for graphic-equalizer realism
          let jitter = Math.random() * 0.25 + 0.78;
          // Dynamic height scaling multiplier reduced by 1/3rd (240 range)
          targetHeight = baseHeight + (currentVolume * 240 * wave * jitter);
        } else if ($status.processing) {
          // Rhythmic scanning ripple sweeping across the bars during processing
          let ripple = Math.sin(time * 0.09 - i * 0.28) * 0.5 + 0.5;
          targetHeight = baseHeight + (ripple * 25.0 * Math.exp(-(centerDist * centerDist) / 180));
        } else {
          // Slow breathing animation when idle (silence)
          let idleWave = Math.sin(time * 0.04 + i * 0.15) * 1.0;
          targetHeight = Math.max(3, baseHeight + idleWave);
        }

        rawHeights.push(targetHeight);
      }

      // Symmetric layout mapping: average height values with their horizontal mirror
      const symmetricHeights: number[] = [];
      for (let i = 0; i < numBars; i++) {
        const mirrorIdx = numBars - 1 - i;
        const avgHeight = (rawHeights[i] + rawHeights[mirrorIdx]) / 2;
        // Clamp height ceiling to 60px (proportional layout reduction)
        symmetricHeights.push(Math.max(3, Math.min(60, avgHeight)));
      }

      barHeights = symmetricHeights;
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
  
  <div class="equalizer-container">
    {#each barHeights as height, i}
      <div 
        class="eq-bar" 
        style="height: {height}px; background: {$status.processing ? `hsl(${190 + (i / 45) * 45}, 82%, 56%)` : `hsl(${285 + (i / 45) * 60}, 76%, 60%)`};"
      ></div>
    {/each}
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

  .equalizer-container {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 2px;
    height: 60px;
    width: 100%;
    margin-top: auto;
  }

  .eq-bar {
    width: 3.5px;
    border-radius: 99px;
    transition: height 0.06s cubic-bezier(0.215, 0.610, 0.355, 1);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.2);
    flex-shrink: 0;
  }
</style>
