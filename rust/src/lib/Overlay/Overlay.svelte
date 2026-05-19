<script lang="ts">
  import { recording, speaking, wordCount } from "../../stores/status";
  import VoiceCard from "./VoiceCard.svelte";
  import Waveform from "./Waveform.svelte";
  import Pulse from "./Pulse.svelte";

  // Overlay style is driven by config; for now default to voice_card
  // In production this reads from the config store
  let overlayStyle = "voice_card";
</script>

<div class="overlay-root" data-recording={$recording} data-speaking={$speaking}>
  {#if $recording || $speaking}
    {#if overlayStyle === "waveform"}
      <Waveform recording={$recording} />
    {:else if overlayStyle === "pulse"}
      <Pulse recording={$recording} />
    {:else}
      <VoiceCard recording={$recording} speaking={$speaking} />
    {/if}
  {/if}
</div>

<style>
  .overlay-root {
    width: 100%;
    height: 100%;
    background: transparent;
    display: flex;
    align-items: center;
    justify-content: center;
    pointer-events: none;
    user-select: none;
  }
</style>
