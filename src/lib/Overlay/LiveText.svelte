<script lang="ts">
  import type { Snippet } from "svelte";
  import { liveTranscript } from "../../stores/liveTranscript";

  /**
   * The live transcript, for the built-in overlays.
   *
   * Shows the newest `lines` lines of the words being transcribed: committed
   * words in the overlay's text color, tentative ones (still subject to
   * correction) dimmer, and a short settle animation when the corrected final
   * transcript replaces the live guess. Until words arrive it shows
   * `placeholder`.
   *
   * Every overlay styles it to fit its own look through these custom
   * properties, set on this element or an ancestor:
   *   --lt-font, --lt-size, --lt-weight, --lt-line-height, --lt-tracking,
   *   --lt-color (committed), --lt-tentative (tentative words),
   *   --lt-placeholder (placeholder text), --lt-flash (final-settle glow),
   *   --lt-fade (height of the fade at the top edge; 0 for none).
   */
  let {
    lines = 2,
    placeholder = "",
    class: cls = "",
    after,
  }: { lines?: number; placeholder?: string; class?: string; after?: Snippet } = $props();

  const committed = $derived($liveTranscript.committed.trim());
  const tentative = $derived($liveTranscript.tentative.trim());
  const empty = $derived(!committed && !tentative);
</script>

<div
  class="lt {cls}"
  class:empty
  class:final={$liveTranscript.final}
  style="--lt-lines: {lines}"
  aria-live="polite"
>
  <div class="lt-flow">
    {#if empty}
      <span class="lt-placeholder">{placeholder}</span>
    {:else}
      {#if committed}<span class="lt-committed">{committed}</span>{/if}{#if committed && tentative}{" "}{/if}{#if tentative}<span class="lt-tentative">{tentative}</span>{/if}
    {/if}
    {#if after}{@render after()}{/if}
  </div>
</div>

<style>
  .lt {
    position: relative;
    height: calc(var(--lt-lines) * var(--lt-line-height, 13px));
    overflow: hidden;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    font-family: var(--lt-font, 'Outfit', 'Inter', sans-serif);
    font-size: var(--lt-size, 10px);
    font-weight: var(--lt-weight, 500);
    line-height: var(--lt-line-height, 13px);
    letter-spacing: var(--lt-tracking, 0.01em);
    color: var(--lt-color, #f3f4f6);
    /* Lines leaving the top fade out instead of being cut mid-glyph. */
    -webkit-mask-image: linear-gradient(to bottom, transparent 0, #000 var(--lt-fade, 6px));
    mask-image: linear-gradient(to bottom, transparent 0, #000 var(--lt-fade, 6px));
  }

  .lt-flow {
    overflow-wrap: anywhere;
    white-space: normal;
  }

  .lt-tentative {
    color: var(--lt-tentative, color-mix(in srgb, currentColor 55%, transparent));
    transition: color 0.25s ease;
  }

  .lt-placeholder {
    color: var(--lt-placeholder, color-mix(in srgb, currentColor 45%, transparent));
    font-style: italic;
  }

  .final .lt-committed {
    animation: lt-settle 0.6s ease-out;
  }

  @keyframes lt-settle {
    0% { text-shadow: 0 0 8px var(--lt-flash, currentColor); }
    100% { text-shadow: 0 0 0 transparent; }
  }
</style>
