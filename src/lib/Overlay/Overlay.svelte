<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { recording, speaking, mcpRecording, status } from "../../stores/status";
  import { config } from "../../stores/config";
  import VoiceCard from "./VoiceCard.svelte";
  import Waveform from "./Waveform.svelte";
  import Pulse from "./Pulse.svelte";
  import BlueWave from "./BlueWave.svelte";

  interface CustomOverlay {
    name: string;
    html: string;
    css: string;
  }

  let visible = $state(true);
  let customOverlays = $state<CustomOverlay[]>([]);
  let activeCustomOverlay = $derived(
    customOverlays.find(o => o.name === $config.ui.overlay_style)
  );

  const triggerLabel = $derived($status.active_target_label || "Focused Window");
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  // Delay unmounting the visualizer when recording/speaking stops to allow CSS outro animation to finish
  let isRecordingOrSpeaking = $derived(
    ($recording && $config.ui.show_overlay) ||
    ($speaking && $config.tts.enabled && $config.tts.response_overlay) ||
    ($mcpRecording && $config.mcp.visual_feedback)
  );
  let renderOverlay = $state(false);
  let animateActive = $state(false);
  let timeoutId: any;
  let animateTimeoutId: any;

  $effect(() => {
    if (isRecordingOrSpeaking) {
      if (timeoutId) clearTimeout(timeoutId);
      renderOverlay = true;
      if (animateTimeoutId) clearTimeout(animateTimeoutId);
      animateTimeoutId = setTimeout(() => {
        animateActive = true;
      }, 25);
    } else {
      animateActive = false;
      timeoutId = setTimeout(() => {
        renderOverlay = false;
      }, 450);
    }
    return () => {
      if (timeoutId) clearTimeout(timeoutId);
      if (animateTimeoutId) clearTimeout(animateTimeoutId);
    };
  });

  let processedHtml = $derived.by(() => {
    if (!activeCustomOverlay) return "";
    return activeCustomOverlay.html
      .replace(/\{\{trigger\}\}/g, triggerLabel)
      .replace(/\{\{target\}\}/g, targetLabel);
  });

  let targetVolume = 0;
  let currentVolume = $state(0);
  let unlistenAudioLevel: (() => void) | null = null;
  let animationFrameId: number;

  $effect(() => {
    // Whenever the overlay style changes, temporarily unmount the visualizer for 1 tick
    // to force the WebKitGTK transparent compositor to completely wipe and flush the old frame buffer
    const _style = $config.ui.overlay_style;
    visible = false;
    const timer = setTimeout(() => {
      visible = true;
    }, 25); // 25ms ensures a full repaint frame ticks in WebKitGTK
    return () => clearTimeout(timer);
  });

  $effect(() => {
    const root = document.documentElement;
    root.style.setProperty("--voxctrl-audio-level", String(currentVolume));
    root.style.setProperty("--voxctrl-recording", $recording ? "1" : "0");
    root.style.setProperty("--voxctrl-processing", $status.processing ? "1" : "0");
    root.style.setProperty("--voxctrl-speaking", $speaking ? "1" : "0");
    root.style.setProperty("--voxctrl-mcp-recording", $mcpRecording ? "1" : "0");
    root.style.setProperty("--voxctrl-audio-ready", $status.audio_ready !== false ? "1" : "0");
  });

  onMount(() => {
    // Add transparent overlay class dynamically to html and body
    document.documentElement.classList.add("overlay-window");
    document.body.classList.add("overlay-window");

    // Force absolute transparency on HTML, body, and App containers to allow Tauri's transparent window to clip correctly
    document.documentElement.style.setProperty("background", "transparent", "important");
    document.body.style.setProperty("background", "transparent", "important");
    
    const appEl = document.getElementById("app");
    if (appEl) {
      appEl.style.setProperty("background", "transparent", "important");
    }

    // Fetch custom overlays from local sharing folder
    invoke<CustomOverlay[]>("get_custom_overlays")
      .then((res) => {
        customOverlays = res;
      })
      .catch((e) => {
        console.error("Failed to load custom overlays:", e);
      });

    // Listen to real-time audio levels from Rust backend
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
      window.dispatchEvent(new CustomEvent("voxctrl-audio-level", { detail: event.payload }));
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    let time = 0;
    function updateAnimation() {
      // Smooth interpolation for visual reaction
      currentVolume += (targetVolume - currentVolume) * 0.42;
      targetVolume *= 0.82;

      // Dispatch high-performance window-level custom events
      if (activeCustomOverlay) {
        window.dispatchEvent(new CustomEvent("voxctrl-status", {
          detail: {
            recording: $recording,
            processing: $status.processing,
            speaking: $speaking,
            audio_ready: $status.audio_ready !== false,
            active_target_label: $status.active_target_label || "Focused Window",
            audio_level: currentVolume,
          }
        }));
      }

      animationFrameId = requestAnimationFrame(updateAnimation);
    }
    animationFrameId = requestAnimationFrame(updateAnimation);

    return () => {
      document.documentElement.classList.remove("overlay-window");
      document.body.classList.remove("overlay-window");
      if (unlistenAudioLevel) unlistenAudioLevel();
      cancelAnimationFrame(animationFrameId);
    };
  });

  // Action to execute scripts dynamically in inserted html
  function executeScripts(node: HTMLElement) {
    const scripts = node.querySelectorAll("script");
    scripts.forEach((oldScript) => {
      const newScript = document.createElement("script");
      Array.from(oldScript.attributes).forEach((attr) => {
        newScript.setAttribute(attr.name, attr.value);
      });
      newScript.appendChild(document.createTextNode(oldScript.innerHTML));
      if (oldScript.parentNode) {
        oldScript.parentNode.replaceChild(newScript, oldScript);
      }
    });

    return {
      destroy() {
        window.dispatchEvent(new CustomEvent("voxctrl-cleanup"));
      }
    };
  }
</script>

<div class="overlay-root" data-recording={$recording} data-speaking={$speaking} data-processing={$status.processing}>
  {#if renderOverlay && visible}
    {#if $config.ui.overlay_style === "waveform"}
      <Waveform recording={$recording} />
    {:else if $config.ui.overlay_style === "pulse"}
      <Pulse recording={$recording} />
    {:else if $config.ui.overlay_style === "blue_wave"}
      <BlueWave recording={$recording} speaking={$speaking} />
    {:else if activeCustomOverlay}
      {@html `<style>${activeCustomOverlay.css}</style>`}
      <div class="custom-overlay-content" class:active={animateActive} use:executeScripts>
        {@html processedHtml}
      </div>
    {:else if $config.ui.overlay_style !== "none"}
      <VoiceCard recording={$recording} speaking={$speaking} />
    {/if}

    {#if $speaking}
      <div class="system-response-box">
        <span class="pulse-dot"></span>
        <span class="label">System Responding...</span>
      </div>
    {:else if $mcpRecording}
      <div class="system-response-box">
        <span class="pulse-dot"></span>
        <span class="label">Recording...</span>
      </div>
    {/if}
  {/if}
</div>

<style>
  :global(html), :global(body), :global(#app) {
    background: transparent !important;
    background-color: transparent !important;
    box-shadow: none !important;
    border: none !important;
    outline: none !important;
    overflow: hidden !important;
    will-change: transform, opacity;
    backface-visibility: hidden;
  }

  :global(*:focus) {
    outline: none !important;
    box-shadow: none !important;
  }

  .overlay-root {
    width: 100%;
    height: 100%;
    background: transparent !important;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    user-select: none;
    overflow: hidden !important;
  }

  .system-response-box {
    position: absolute;
    z-index: 10;
    width: 260px;
    height: 64px;
    background: rgba(239, 68, 68, 0.88);
    border: 2px solid rgba(239, 68, 68, 0.85);
    box-shadow: 0 8px 32px rgba(239, 68, 68, 0.3), inset 0 0 12px rgba(255, 255, 255, 0.2);
    border-radius: 12px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 12px;
    color: #ffffff;
    font-family: 'Inter', system-ui, sans-serif;
    font-weight: 700;
    font-size: 14px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    transition: all 0.3s cubic-bezier(0.16, 1, 0.3, 1);
    animation: popIn 0.35s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .pulse-dot {
    width: 10px;
    height: 10px;
    background-color: #ffffff;
    border-radius: 50%;
    animation: flash 1.2s infinite ease-in-out;
  }

  @keyframes popIn {
    0% { transform: scale(0.85); opacity: 0; }
    100% { transform: scale(1); opacity: 1; }
  }

  @keyframes flash {
    0%, 100% { opacity: 0.3; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.15); }
  }
</style>
