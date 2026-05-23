<script lang="ts">
  import { onMount } from "svelte";
  import { recording, speaking, status } from "../../stores/status";
  import { config } from "../../stores/config";
  import VoiceCard from "./VoiceCard.svelte";
  import Waveform from "./Waveform.svelte";
  import Pulse from "./Pulse.svelte";

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

    return () => {
      document.documentElement.classList.remove("overlay-window");
      document.body.classList.remove("overlay-window");
    };
  });
</script>

<div class="overlay-root" data-recording={$recording} data-speaking={$speaking} data-processing={$status.processing}>
  {#if $recording || $speaking || $status.processing}
    {#if $config.ui.overlay_style === "waveform"}
      <Waveform recording={$recording} />
    {:else if $config.ui.overlay_style === "pulse"}
      <Pulse recording={$recording} />
    {:else if $config.ui.overlay_style !== "none"}
      <VoiceCard recording={$recording} speaking={$speaking} />
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
  }
</style>
