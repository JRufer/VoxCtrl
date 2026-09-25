<!--
  How global shortcuts are reaching VoxCtrl, and — on KDE — the manual step
  its portal leaves the user. Part of Settings → Hotkeys.
-->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { HotkeyStatus } from "./hotkeys";

  let { status }: { status: HotkeyStatus | null } = $props();

  let backendBannerExpanded = $state(false);
  let manualEnableExpanded = $state(false);
  let openingShortcutSettings = $state(false);
  let openShortcutSettingsError = $state<string | null>(null);

  async function openShortcutSettings() {
    openingShortcutSettings = true;
    openShortcutSettingsError = null;
    try {
      await invoke("open_shortcut_settings");
    } catch (e: any) {
      openShortcutSettingsError = e?.toString() ?? "Could not open shortcut settings.";
    } finally {
      openingShortcutSettings = false;
    }
  }
</script>

{#if status}
  <div
    class="backend-banner"
    class:private={status.is_private}
    class:warn={!status.is_private && status.is_active}
    class:broken={!status.is_active}
  >
    <button
      type="button"
      class="banner-toggle"
      onclick={() => backendBannerExpanded = !backendBannerExpanded}
      aria-expanded={backendBannerExpanded}
      aria-label="Toggle shortcut backend details"
    >
      <span class="backend-icon">
        {status.is_private ? "🔒" : status.is_active ? "⚠️" : "⛔"}
      </span>
      <strong class="banner-title">
        {#if status.backend === "portal"}
          Your desktop is handling these shortcuts
        {:else if status.backend === "windows_hook"}
          Reading keystrokes with a keyboard hook
        {:else if status.backend === "evdev"}
          Reading input devices directly
        {:else if status.backend === "starting"}
          Starting up…
        {:else}
          Global shortcuts are not available
        {/if}
      </strong>
      <svg
        class="banner-chevron"
        class:expanded={backendBannerExpanded}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </button>
    {#if backendBannerExpanded}
      <div class="banner-content">
        <span class="backend-detail">{status.detail}</span>
        {#if status.backend === "portal"}
          <span class="backend-detail">
            Your desktop decides which keys VoxCtrl may claim, and may ask you to confirm them.
            If a shortcut below shows different keys than you chose, that is your desktop's
            choice and it wins.
          </span>
        {/if}
      </div>
    {/if}
  </div>
{/if}

{#if status?.needs_manual_enable}
  <div class="manual-enable-banner">
    <button
      type="button"
      class="banner-toggle"
      onclick={() => manualEnableExpanded = !manualEnableExpanded}
      aria-expanded={manualEnableExpanded}
      aria-label="Toggle manual shortcut enable details"
    >
      <span class="backend-icon">🔧</span>
      <strong class="banner-title">One more step on KDE: enable these shortcuts yourself</strong>
      <svg
        class="banner-chevron"
        class:expanded={manualEnableExpanded}
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        stroke-width="2"
        stroke-linecap="round"
        stroke-linejoin="round"
      >
        <polyline points="6 9 12 15 18 9"></polyline>
      </svg>
    </button>
    {#if manualEnableExpanded}
      <div class="banner-content">
        <span class="backend-detail">{status.manual_enable_hint}</span>
        <div class="manual-enable-actions">
          <button
            class="btn-action primary"
            onclick={openShortcutSettings}
            disabled={openingShortcutSettings}
          >
            {openingShortcutSettings ? "Opening…" : "Open Shortcut Settings"}
          </button>
        </div>
        {#if openShortcutSettingsError}
          <span class="backend-detail error">{openShortcutSettingsError}</span>
        {/if}
      </div>
    {/if}
  </div>
{/if}

<style lang="postcss">
  @reference "../../app.css";

  .backend-banner {
    @apply flex flex-col rounded-[var(--radius)] p-2.5 px-3.5 mb-1 border bg-white/[0.03] border-[var(--border)] transition-colors duration-150;
  }

  .backend-banner.private {
    @apply bg-emerald-500/6 border-emerald-500/25;
  }

  .backend-banner.warn {
    @apply bg-amber-500/6 border-amber-500/25;
  }

  .backend-banner.broken {
    @apply bg-red-500/6 border-red-500/25;
  }

  .manual-enable-banner {
    @apply flex flex-col rounded-[var(--radius)] p-2.5 px-3.5 mb-1 border bg-amber-500/6 border-amber-500/25 transition-colors duration-150;
  }

  .banner-toggle {
    @apply flex items-center gap-2.5 w-full bg-transparent border-none p-0 text-left cursor-pointer select-none text-[var(--text)];
  }

  .banner-title {
    @apply flex-1 text-[12.5px] font-semibold leading-normal;
  }

  .banner-chevron {
    @apply w-4 h-4 text-[var(--text-muted)] shrink-0 transition-transform duration-200 ease-in-out;
  }

  .banner-chevron.expanded {
    @apply rotate-180 text-[var(--text)];
  }

  .banner-content {
    @apply flex flex-col gap-1 min-w-0 w-full text-[12.5px] mt-2 pt-2 border-t border-white/5 pl-6.5;
  }

  .backend-icon {
    @apply text-base leading-none shrink-0;
  }

  .backend-detail {
    @apply text-[var(--text-muted)] leading-relaxed;
  }

  .backend-detail.error {
    @apply text-red-400;
  }

  .manual-enable-actions {
    @apply mt-1;
  }
</style>
