<script lang="ts">
  import { gpuLabel } from "./gpu";

  /** Every engine's GPU switch is this one control, so acceleration is turned
   *  on the same way everywhere and reads the same in every tab. */
  let {
    backend,
    on,
    onchange,
    hint = "",
  } = $props<{
    /** The GPU backend this build has for the engine, or null when it has none. */
    backend: string | null;
    /** Whether the engine is set to use it. */
    on: boolean;
    onchange: (on: boolean) => void;
    /** Extra explanation, shown under the switch when a GPU path exists. */
    hint?: string;
  }>();
</script>

<label class="field">
  <span>GPU acceleration{backend ? ` (${gpuLabel(backend)})` : ""}</span>
  <input
    type="checkbox"
    checked={on && !!backend}
    disabled={!backend}
    onchange={(e) => onchange(e.currentTarget.checked)}
  />
</label>
<p class="hint">
  {#if !backend}
    This build has no GPU path for this engine; it runs on the CPU.
  {:else if hint}
    {hint}
  {:else}
    Off runs this engine on the CPU. Takes effect the next time the model loads.
  {/if}
</p>
