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
  import MonoBars from "./MonoBars.svelte";
  import Spectrum from "./Spectrum.svelte";
  import Terminal from "./Terminal.svelte";
  import Vinyl from "./Vinyl.svelte";

  interface CustomOverlay {
    name: string;
    html: string;
    css: string;
  }

  // Names of the built-in styles Overlay.svelte itself renders (see the
  // {#if overlay-style === ...} chain in the markup below), plus "none" —
  // anything outside this set is looked up as a custom overlay folder.
  const BUILTIN_STYLES = new Set([
    "waveform", "pulse", "blue_wave", "mono_bars", "spectrum", "terminal", "vinyl", "voice_card", "none",
  ]);

  let visible = $state(true);
  let activeCustomOverlay = $state<CustomOverlay | undefined>(undefined);

  const triggerLabel = $derived($status.active_target_label || "Focused Window");
  const targetLabel = $derived($status.active_target_label || "Focused Window");

  let commandOverlayActive = $state(false);
  let commandOverlayName = $state("");
  let commandOverlayText = $state("");
  let commandTimerId: any = null;
  let unlistenCommandExecuted: (() => void) | null = null;
  let unlistenOverlayStyleSelected: (() => void) | null = null;

  // Delay unmounting the visualizer when recording/speaking/command stops to allow CSS outro animation to finish
  let isRecordingOrSpeaking = $derived(
    ($recording && $config.ui.show_overlay) ||
    ($speaking && $config.tts.enabled && $config.tts.response_overlay) ||
    ($mcpRecording && $config.mcp.visual_feedback) ||
    (commandOverlayActive && $config.ui.show_command_overlay)
  );
  let renderOverlay = $state(false);
  let animateActive = $state(false);
  let timeoutId: any;
  let animateTimeoutId: any;

  $effect(() => {
    if (isRecordingOrSpeaking) {
      if (timeoutId) clearTimeout(timeoutId);
      renderOverlay = true;
      // Re-map the overlay window before it has anything to show. Nothing
      // short of actually unmapping the window (see the idle branch below)
      // reliably clears a stuck frame on some systems, so the window is no
      // longer left mapped for the app's whole session — it comes back
      // here every time there's something to display.
      invoke("show_overlay_window").catch(() => {});
      if (animateTimeoutId) clearTimeout(animateTimeoutId);
      animateTimeoutId = setTimeout(() => {
        animateActive = true;
      }, 25);
      // See loadActiveCustomOverlay's doc comment below: this is the
      // trigger that actually matters for "I edited the file, does the
      // next activation show it" — re-read on every activation, not just
      // when the style value itself happens to change.
      loadActiveCustomOverlay($config.ui.overlay_style);
    } else {
      animateActive = false;
      timeoutId = setTimeout(() => {
        renderOverlay = false;
        // Actually unmap the window rather than trying to make WebKitGTK
        // repaint it to blank: forcing a repaint (resizing/moving the
        // window, mapping an extra window, changing its X11 window-type
        // hint) was tried and none of it reliably cleared a stuck frame.
        // An unmapped window has nothing for the compositor to display,
        // stale buffer or not.
        invoke("hide_overlay_window").catch(() => {});
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
  let animationFrameId: number | null = null;

  /**
   * Drive the level smoothing while there is something to smooth.
   *
   * The loop used to run for the lifetime of the window, so an idle VoxCtrl
   * kept a 60 Hz callback alive — writing a CSS variable and forcing a style
   * recalculation every frame — for a volume that had decayed to a rounding
   * error. It now stops once the level has settled at zero and nothing is on
   * screen, and any new level restarts it.
   */
  function startAnimation() {
    if (animationFrameId !== null) return;
    animationFrameId = requestAnimationFrame(updateAnimation);
  }

  function updateAnimation() {
    // Smooth interpolation for visual reaction
    currentVolume += (targetVolume - currentVolume) * 0.42;
    targetVolume *= 0.82;
    // Snap the tail of the decay to zero so "settled" is a state the loop can
    // actually reach instead of an asymptote.
    if (targetVolume < 0.0005) targetVolume = 0;
    if (currentVolume < 0.0005) currentVolume = 0;

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

    if (currentVolume === 0 && targetVolume === 0 && !renderOverlay) {
      animationFrameId = null;
      return;
    }
    animationFrameId = requestAnimationFrame(updateAnimation);
  }

  // Anything that puts the overlay on screen also needs the loop running, even
  // before the first level arrives (a custom overlay reads its status from the
  // events the loop dispatches).
  $effect(() => {
    if (renderOverlay) startAnimation();
  });

  // A custom overlay's files are read fresh from disk right here — both
  // index.html and style.css together, in one call — not once at app
  // startup, and not cached in between, so editing either file always
  // shows up on the very next read with no app restart needed. This
  // window is created once and stays alive for the app's whole session
  // (see window::open_overlay_window), so without re-reading on demand
  // like this, whatever was on disk at startup is all it would ever show.
  //
  // Reading it again is triggered three different ways:
  //   1. The "overlay-style-selected" listener below, fired directly by
  //      Settings on every selection in the Overlay style dropdown —
  //      including re-selecting the style that's already active. This is
  //      the reliable one for "I edited the file, does picking this style
  //      show it right now": it bypasses the config store entirely, so it
  //      isn't subject to point 3 below.
  //   2. The isRecordingOrSpeaking effect above, on every activation (a
  //      dictation starts) — the guarantee for "will the next dictation
  //      show my edit", independent of anything Settings did.
  //   3. The style-change effect further down, which reacts to
  //      config.ui.overlay_style itself changing. Kept as a fallback for
  //      config changes that don't originate from that dropdown (e.g. a
  //      config file edited by hand), but not relied on alone: this
  //      window's copy of that value only updates when it *receives* a
  //      config-changed event with a different value than it already had,
  //      and Settings auto-saves on a debounce — two quick changes can
  //      collapse into one save equal to the original value, which this
  //      window never sees as a change at all.
  function loadActiveCustomOverlay(style: string) {
    if (BUILTIN_STYLES.has(style)) {
      activeCustomOverlay = undefined;
      return;
    }
    invoke<CustomOverlay | null>("get_custom_overlay", { name: style })
      .then((res) => {
        activeCustomOverlay = res ?? undefined;
      })
      .catch((e) => {
        console.error(`Failed to load custom overlay "${style}":`, e);
        activeCustomOverlay = undefined;
      });
  }

  $effect(() => {
    // Whenever the overlay style changes, temporarily unmount the visualizer for 1 tick
    // to force the WebKitGTK transparent compositor to completely wipe and flush the old frame buffer
    const style = $config.ui.overlay_style;
    visible = false;
    const timer = setTimeout(() => {
      visible = true;
    }, 25); // 25ms ensures a full repaint frame ticks in WebKitGTK

    loadActiveCustomOverlay(style);

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

    // Custom overlays are fetched in the $effect above, which also runs once
    // on mount (and again on every style switch, so on-disk edits show up
    // without an app restart).

    // Fired directly by Settings (VisualTab.svelte) on every Overlay style
    // dropdown selection — see loadActiveCustomOverlay's doc comment above
    // for why this exists alongside the config-driven effect.
    listen<string>("overlay-style-selected", (event) => {
      loadActiveCustomOverlay(event.payload);
    }).then((unlisten) => {
      unlistenOverlayStyleSelected = unlisten;
    });

    // Listen to real-time audio levels from Rust backend
    listen<number>("audio-level", (event) => {
      targetVolume = Math.min(1.0, event.payload * 100.0);
      startAnimation();
      window.dispatchEvent(new CustomEvent("voxctrl-audio-level", { detail: event.payload }));
    }).then((unlisten) => {
      unlistenAudioLevel = unlisten;
    });

    listen<{ command: string; summary: string; duration_secs: number }>("command-executed", (event) => {
      if (!$config.ui.show_command_overlay) return;
      commandOverlayName = event.payload.command;
      commandOverlayText = event.payload.summary;
      commandOverlayActive = true;
      if (commandTimerId) clearTimeout(commandTimerId);
      const durationMs = (event.payload.duration_secs || $config.ui.command_overlay_duration_secs || 3) * 1000;
      commandTimerId = setTimeout(() => {
        commandOverlayActive = false;
      }, durationMs);
    }).then((unlisten) => {
      unlistenCommandExecuted = unlisten;
    });

    return () => {
      document.documentElement.classList.remove("overlay-window");
      document.body.classList.remove("overlay-window");
      if (unlistenAudioLevel) unlistenAudioLevel();
      if (unlistenCommandExecuted) unlistenCommandExecuted();
      if (unlistenOverlayStyleSelected) unlistenOverlayStyleSelected();
      if (commandTimerId) clearTimeout(commandTimerId);
      if (animationFrameId !== null) cancelAnimationFrame(animationFrameId);
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
      <Waveform recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "pulse"}
      <Pulse recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "blue_wave"}
      <BlueWave recording={$recording} speaking={$speaking} active={animateActive} />
    {:else if $config.ui.overlay_style === "mono_bars"}
      <MonoBars recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "spectrum"}
      <Spectrum recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "terminal"}
      <Terminal recording={$recording} active={animateActive} />
    {:else if $config.ui.overlay_style === "vinyl"}
      <Vinyl recording={$recording} active={animateActive} />
    {:else if activeCustomOverlay}
      {@html `<style>${activeCustomOverlay.css}</style>`}
      <div class="custom-overlay-content" class:active={animateActive} use:executeScripts>
        {@html processedHtml}
      </div>
    {:else if $config.ui.overlay_style !== "none"}
      <VoiceCard recording={$recording} speaking={$speaking} active={animateActive} />
    {/if}

    {#if $speaking}
      <div class="system-response-box speaking" class:on={animateActive}>
        <span class="mini-eq">
          {#each [0, 1, 2, 3, 4] as i}
            <span class="eq-bar" style="animation-delay: {i * 0.13}s"></span>
          {/each}
        </span>
        <span class="pill-text">
          <span class="pill-title">SYSTEM RESPONDING</span>
          <span class="pill-target">▸ {targetLabel}</span>
        </span>
      </div>
    {:else if commandOverlayActive}
      <div class="system-response-box command" class:on={animateActive}>
        <span class="cmd-icon">⚡</span>
        <span class="pill-text">
          <span class="pill-title">{commandOverlayName.toUpperCase()}</span>
          <span class="pill-target">▸ {commandOverlayText}</span>
        </span>
      </div>
    {:else if $mcpRecording}
      <div class="system-response-box mcp" class:on={animateActive}>
        <span class="pulse-dot"></span>
        <span class="pill-text">
          <span class="pill-title">RECORDING</span>
          <span class="pill-target">▸ {targetLabel}</span>
        </span>
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
    display: flex;
    align-items: center;
    gap: 10px;
    height: 46px;
    padding: 0 20px;
    border-radius: 23px;
    font-family: 'Outfit', 'Inter', system-ui, sans-serif;
    opacity: 0;
    transform: translateY(20px);
    transition:
      transform 0.34s cubic-bezier(0.175, 0.885, 0.32, 1.25),
      opacity 0.28s ease;
  }

  .system-response-box.on {
    opacity: 1;
    transform: translateY(0);
  }

  .system-response-box.speaking {
    background: rgba(4, 47, 36, 0.94);
    border: 1.2px solid rgba(16, 185, 129, 0.55);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5), 0 0 18px rgba(16, 185, 129, 0.25);
    color: #a7f3d0;
  }

  .system-response-box.command {
    background: rgba(30, 16, 60, 0.94);
    border: 1.2px solid rgba(168, 85, 247, 0.65);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5), 0 0 18px rgba(168, 85, 247, 0.3);
    color: #e9d5ff;
  }

  .system-response-box.mcp {
    background: rgba(76, 5, 25, 0.94);
    border: 1.2px solid rgba(244, 63, 94, 0.55);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.5), 0 0 18px rgba(244, 63, 94, 0.25);
    color: #fecdd3;
  }

  .cmd-icon {
    font-size: 16px;
    line-height: 1;
    color: #c084fc;
    animation: flash 1.5s infinite ease-in-out;
  }

  .mini-eq {
    display: flex;
    align-items: center;
    gap: 2.5px;
    height: 22px;
  }

  .eq-bar {
    width: 3px;
    height: 8px;
    border-radius: 1.5px;
    background: #34d399;
    animation: eq-bounce 0.8s ease-in-out infinite alternate;
  }

  @keyframes eq-bounce {
    from { height: 7px; }
    to { height: 21px; }
  }

  .pill-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  .pill-title {
    font-size: 10px;
    font-weight: 850;
    letter-spacing: 0.15em;
  }

  .pill-target {
    font-size: 8.5px;
    font-weight: 600;
    opacity: 0.55;
    max-width: 180px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .pulse-dot {
    width: 10px;
    height: 10px;
    background-color: #f43f5e;
    border-radius: 50%;
    animation: flash 1.2s infinite ease-in-out;
  }

  @keyframes flash {
    0%, 100% { opacity: 0.3; transform: scale(0.9); }
    50% { opacity: 1; transform: scale(1.15); }
  }
</style>
