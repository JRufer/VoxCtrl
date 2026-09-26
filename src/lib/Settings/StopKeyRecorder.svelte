<!--
  Records the key combination that stops TTS playback (Settings → TTS).
  Click or focus it, press the combination, release: the keys held at release
  become the new binding.
-->
<script lang="ts">
  import { mapBrowserKeyToEvdev } from "../keys";

  let {
    keys = $bindable(),
    onchange,
  }: {
    keys: string[];
    onchange?: () => void;
  } = $props();

  let isRecordingStopKey = $state(false);
  let currentlyPressedStopKeys = $state<string[]>([]);

  function handleStopKeyDown(e: KeyboardEvent) {
    if (!isRecordingStopKey) return;
    e.preventDefault();
    e.stopPropagation();
    const evdevKey = mapBrowserKeyToEvdev(e.key, e.code);
    if (!currentlyPressedStopKeys.includes(evdevKey)) {
      currentlyPressedStopKeys = [...currentlyPressedStopKeys, evdevKey];
    }
    // Escape triggers browser blur before keyup fires, so commit immediately
    // on keydown for single-key combos where Escape is the key pressed.
    // For multi-key combos, keyup still handles commit as normal.
    if (e.key === "Escape") {
      keys = [...currentlyPressedStopKeys];
      onchange?.();
      currentlyPressedStopKeys = [];
      isRecordingStopKey = false;
    }
  }

  function handleStopKeyUp(e: KeyboardEvent) {
    if (!isRecordingStopKey) return;
    e.preventDefault();
    e.stopPropagation();
    if (currentlyPressedStopKeys.length > 0) {
      keys = [...currentlyPressedStopKeys];
      onchange?.();
    }
    currentlyPressedStopKeys = [];
    isRecordingStopKey = false;
  }

  function handleStopKeyBlur() {
    // Safety net: if blur fires while we have pending keys (e.g. Escape blur race),
    // commit whatever was captured rather than discarding it silently.
    if (currentlyPressedStopKeys.length > 0) {
      keys = [...currentlyPressedStopKeys];
      onchange?.();
      currentlyPressedStopKeys = [];
    }
    isRecordingStopKey = false;
  }
</script>

<div
  class={[
    "border-2 rounded-desktop p-6 text-center cursor-pointer outline-none transition-all duration-200 flex flex-col items-center justify-center min-h-[80px]",
    isRecordingStopKey
      ? "border-solid border-[#f43f5e] bg-[rgba(244,63,94,0.05)] animate-border-pulse"
      : "border-dashed border-white/5 bg-black/25 hover:border-accent-blue hover:bg-black/35 focus:border-accent-blue focus:bg-black/35"
  ].join(" ")}
  tabindex="0"
  role="button"
  aria-label="Stop key recorder"
  onclick={() => isRecordingStopKey = true}
  onfocus={() => isRecordingStopKey = true}
  onblur={handleStopKeyBlur}
  onkeydown={handleStopKeyDown}
  onkeyup={handleStopKeyUp}
>
  {#if isRecordingStopKey}
    <div class="flex items-center gap-[10px]">
      <span class="w-2 h-2 bg-accent-blue rounded-full animate-flash"></span>
      <span class="text-[13px] font-semibold text-accent-blue">
        {currentlyPressedStopKeys.length > 0
          ? currentlyPressedStopKeys.join(" + ").replace(/KEY_/g, "")
          : "Press your physical shortcut combination now..."}
      </span>
    </div>
  {:else}
    <span class="text-[12px] text-obsidian-300 flex flex-col gap-2 items-center">
      {#if keys.length > 0}
        <div class="flex gap-1.5">
          {#each keys as k}
            <kbd class="px-1.5! py-0.5! text-[12px] bg-accent-blue text-black border-0 font-extrabold rounded">{k.replace("KEY_", "")}</kbd>
          {/each}
        </div>
        <span class="text-[10px] text-accent-blue opacity-80">(Click / Tab here to record a new stop key)</span>
      {:else}
        ⚠️ Click/Focus here to press a stop key!
      {/if}
    </span>
  {/if}
</div>
