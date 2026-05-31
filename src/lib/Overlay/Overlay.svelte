<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { recording, speaking, status } from "../../stores/status";
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
    const root = document.querySelector(".overlay-root") as HTMLElement;
    if (root) {
      root.style.setProperty("--voxctrl-audio-level", String(currentVolume));
      root.style.setProperty("--voxctrl-recording", $recording ? "1" : "0");
      root.style.setProperty("--voxctrl-processing", $status.processing ? "1" : "0");
      root.style.setProperty("--voxctrl-speaking", $speaking ? "1" : "0");
      root.style.setProperty("--voxctrl-audio-ready", $status.audio_ready !== false ? "1" : "0");
    }
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
  }
</script>

<div class="overlay-root" data-recording={$recording} data-speaking={$speaking} data-processing={$status.processing}>
  {#if ($recording || $speaking || $status.processing) && visible}
    {#key $config.ui.overlay_style}
      {#if $config.ui.overlay_style === "waveform"}
        <Waveform recording={$recording} />
      {:else if $config.ui.overlay_style === "pulse"}
        <Pulse recording={$recording} />
      {:else if $config.ui.overlay_style === "blue_wave"}
        <BlueWave recording={$recording} speaking={$speaking} />
      {:else if activeCustomOverlay}
        {@html `<style>${activeCustomOverlay.css}</style>`}
        <div class="custom-overlay-content" use:executeScripts>
          {@html processedHtml}
        </div>
      {:else if $config.ui.overlay_style !== "none"}
        <VoiceCard recording={$recording} speaking={$speaking} />
      {/if}
      {/key}
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
</style>
