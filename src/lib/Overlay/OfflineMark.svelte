<script lang="ts">
  import { status } from "../../stores/status";

  /**
   * The offline mark, for the built-in overlays.
   *
   * Shown only while a remote speech engine is selected and its server cannot
   * be reached (`status.remote_offline`): the dictation on screen is running
   * on the fallback engine, or is about to fail. Otherwise it renders nothing,
   * so every style looks exactly as it did before.
   *
   * It inherits the font, size and letter-spacing of wherever it is placed,
   * so it reads as part of each style's own status line; only the amber is
   * shared, to match the tray icon's offline badge. A style adjusts it with
   * these custom properties, set on the component or an ancestor:
   *   --om-color (default amber), --om-gap (space before it),
   *   --om-dot (the lamp's `display`; `none` for text only),
   *   --om-size, --om-weight, --om-opacity.
   */
  let { text = "OFFLINE", class: cls = "" }: { text?: string; class?: string } = $props();
</script>

{#if $status.remote_offline}
  <span class="om {cls}" role="status" title="The speech server can't be reached">{text}</span>
{/if}

<style>
  .om {
    display: inline-flex;
    align-items: center;
    gap: 0.45em;
    margin-left: var(--om-gap, 0.7em);
    font: inherit;
    font-size: var(--om-size, inherit);
    font-weight: var(--om-weight, inherit);
    letter-spacing: inherit;
    text-transform: inherit;
    white-space: nowrap;
    color: var(--om-color, #f59e0b);
    opacity: var(--om-opacity, 1);
  }

  .om::before {
    content: "";
    display: var(--om-dot, inline-block);
    width: 0.55em;
    height: 0.55em;
    border-radius: 50%;
    background: currentColor;
    box-shadow: 0 0 0.5em color-mix(in srgb, currentColor 60%, transparent);
  }
</style>
