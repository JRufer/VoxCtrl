<script lang="ts">
  import { gpuLabel } from "./gpu";

  /** The corner chip every engine shows: model state, and — whenever the GPU is
   *  actually in use — which GPU path. */
  let {
    state,
    backend,
    on,
  } = $props<{
    state: "ready" | "downloading" | "missing" | "checking" | "device";
    /** The GPU backend this build has for the engine, or null. */
    backend: string | null;
    /** Whether the engine is set to use it. */
    on: boolean;
  }>();

  const device = $derived(backend && on ? gpuLabel(backend) : "CPU");
</script>

{#if state === "device"}
  <span class="device-chip" class:success={backend && on}>{backend && on ? `⚡ ${device}` : "CPU"}</span>
{:else if state === "ready"}
  <span class="device-chip success" class:gpu={backend && on}>✔ Ready ({device})</span>
{:else if state === "downloading"}
  <span class="device-chip downloading">⏳ Downloading</span>
{:else if state === "missing"}
  <span class="device-chip error">Missing</span>
{/if}

<style lang="postcss">
  @reference "../../app.css";
  .device-chip {
    @apply text-xs px-3 py-1.5 rounded-[var(--radius)] font-medium leading-normal;
  }
  .device-chip.success {
    @apply bg-emerald-500/15 text-emerald-300 border border-emerald-500/30;
  }
  .device-chip:not(.success):not(.error):not(.downloading) {
    @apply bg-slate-500/15 text-slate-300 border border-slate-500/30;
  }
  .device-chip.error {
    @apply bg-red-500/15 text-red-300 border border-red-500/30;
  }
  .device-chip.downloading {
    @apply bg-cyan-500/15 text-cyan-300 border border-cyan-500/30;
  }
</style>
