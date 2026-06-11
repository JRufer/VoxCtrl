<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { status } from "../../stores/status";

  let { recording = false, active = true } = $props();

  let level = $state(0);
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

    function update() {
      currentVolume += (targetVolume - currentVolume) * 0.35;
      targetVolume *= 0.86;
      level = currentVolume;
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
  class="radar"
  class:on={active}
  class:processing={$status.processing}
  class:initializing={recording && !isReady}
  style="--lvl: {level}"
>
  <!-- Sonar dial -->
  <div class="dial">
    <div class="range-ring r1"></div>
    <div class="range-ring r2"></div>
    <div class="axis h"></div>
    <div class="axis v"></div>
    <div class="sweep"></div>
    <div class="pulse-ring p1"></div>
    <div class="pulse-ring p2"></div>
    <div class="blip b1"></div>
    <div class="blip b2"></div>
    <div class="core"></div>
  </div>

  <!-- Target-lock plate -->
  <div class="plate">
    <span class="reticle">⌖</span>
    <span class="plate-text">
      <span class="caption">
        {#if $status.processing}
          PULSE // ANALYZING
        {:else if recording && !isReady}
          PULSE // ACQUIRING
        {:else}
          PULSE // TARGET LOCK
        {/if}
      </span>
      <span class="label">
        {#if $status.processing}
          Decoding transmission…
        {:else if recording && !isReady}
          Connecting mic…
        {:else}
          {targetLabel}
        {/if}
      </span>
    </span>
  </div>
</div>

<style>
  .radar {
    --tone: #ff6b35;
    --tone-soft: rgba(255, 107, 53, 0.4);
    display: flex;
    align-items: center;
    gap: 24px;
    pointer-events: none;
    user-select: none;
  }

  .radar.initializing {
    --tone: #f59e0b;
    --tone-soft: rgba(245, 158, 11, 0.4);
  }

  .radar.processing {
    --tone: #38bdf8;
    --tone-soft: rgba(56, 189, 248, 0.4);
  }

  /* ── Dial: drops in on load, lifts away on unload ─────────── */
  .dial {
    position: relative;
    width: 124px;
    height: 124px;
    border-radius: 50%;
    background: radial-gradient(circle, #10181c 0%, #06090b 85%, #04070a 100%);
    border: 1.2px solid var(--tone-soft);
    box-shadow: 0 12px 34px rgba(0, 0, 0, 0.55), 0 0 22px color-mix(in srgb, var(--tone) 20%, transparent);
    overflow: hidden;
    transform: translateY(30px) scale(0.85);
    opacity: 0;
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.25),
      opacity 0.28s ease;
  }

  .on .dial {
    transform: translateY(0) scale(1);
    opacity: 1;
  }

  .range-ring {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    border-radius: 50%;
    border: 1px solid rgba(255, 255, 255, 0.1);
  }
  .r1 { width: 92px; height: 92px; }
  .r2 { width: 56px; height: 56px; border-color: rgba(255, 255, 255, 0.14); }

  .axis {
    position: absolute;
    background: rgba(255, 255, 255, 0.07);
  }
  .axis.h { left: 0; right: 0; top: 50%; height: 1px; }
  .axis.v { top: 0; bottom: 0; left: 50%; width: 1px; }

  /* Rotating sweep beam */
  .sweep {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background: conic-gradient(
      from 0deg,
      transparent 0deg,
      transparent 300deg,
      color-mix(in srgb, var(--tone) 35%, transparent) 350deg,
      var(--tone) 360deg
    );
    animation: sweep-rotate 1.5s linear infinite;
    opacity: 0.7;
  }

  @keyframes sweep-rotate {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }

  /* Expanding audio pulse rings — louder voice, brighter rings */
  .pulse-ring {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 118px;
    height: 118px;
    margin: -59px 0 0 -59px;
    border-radius: 50%;
    border: 1.5px solid var(--tone);
    opacity: 0;
    animation: ring-expand 1.4s cubic-bezier(0.1, 0.7, 0.3, 1) infinite;
  }
  .p2 { animation-delay: 0.7s; }

  @keyframes ring-expand {
    0% { transform: scale(0.18); opacity: calc(0.25 + var(--lvl) * 0.65); }
    100% { transform: scale(1); opacity: 0; }
  }

  /* Contact blips lit by the passing sweep */
  .blip {
    position: absolute;
    border-radius: 50%;
    background: color-mix(in srgb, var(--tone) 60%, #ffffff);
    animation: blip-flash 1.5s linear infinite;
  }
  .b1 { left: 88px; top: 42px; width: 5px; height: 5px; }
  .b2 { left: 36px; top: 86px; width: 4px; height: 4px; animation-delay: 0.55s; }

  @keyframes blip-flash {
    0% { opacity: 1; }
    55% { opacity: 0.05; }
    100% { opacity: 0.05; }
  }

  /* Audio-reactive core */
  .core {
    position: absolute;
    top: 50%;
    left: 50%;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border-radius: 50%;
    background: var(--tone);
    box-shadow: 0 0 14px var(--tone);
    transform: scale(calc(1 + var(--lvl) * 1.6));
    transition: transform 0.05s linear;
  }

  /* ── Target-lock plate: slides out from the dial ──────────── */
  .plate {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 200px;
    height: 52px;
    padding: 0 12px;
    background: rgba(10, 12, 16, 0.94);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: 10px;
    position: relative;
    transform: translateX(-22px);
    opacity: 0;
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.25) 0.06s,
      opacity 0.28s ease 0.06s;
  }

  .on .plate {
    transform: translateX(0);
    opacity: 1;
  }

  .plate::after {
    content: "";
    position: absolute;
    inset: -1px;
    border-radius: 10px;
    border: 1px solid var(--tone);
    animation: lock-pulse 1.15s ease-in-out infinite;
    pointer-events: none;
  }

  @keyframes lock-pulse {
    0%, 100% { opacity: 0.25; }
    50% { opacity: 0.85; }
  }

  .reticle {
    color: var(--tone);
    font-size: 22px;
    line-height: 1;
    animation: lock-pulse 1.15s ease-in-out infinite;
  }

  .plate-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    font-family: 'Outfit', 'Inter', sans-serif;
  }

  .caption {
    color: rgba(255, 255, 255, 0.35);
    font-size: 7.5px;
    font-weight: 800;
    letter-spacing: 0.18em;
  }

  .label {
    color: #e5e7eb;
    font-size: 12px;
    font-weight: 750;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
