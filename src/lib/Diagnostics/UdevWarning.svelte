<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { open } from "@tauri-apps/plugin-shell";

  let udevStatus = $state<{
    is_configured: boolean;
    rule_exists: boolean;
    in_group: boolean;
    needs_relogin: boolean;
  } | null>(null);

  onMount(async () => {
    try {
      const status: any = await invoke("check_udev_status");
      udevStatus = status;
    } catch (err) {
      console.error("Failed to check udev status in diagnostics window:", err);
    }
  });

  async function handleClose() {
    try {
      const window = getCurrentWindow();
      await window.close();
    } catch (err) {
      console.error("Failed to close window natively:", err);
    }
  }

  async function handleDownload() {
    try {
      await open("https://github.com/JRufer/VoxCtr/blob/master/install.sh");
    } catch (err) {
      console.error("Failed to open install.sh URL:", err);
    }
  }
</script>

<div class="diagnostic-window">
  {#if udevStatus}
    <div class="modal-card">
      <div class="modal-icon">⚠️</div>
      <h2 class="modal-title">Hardware Permissions Required</h2>
      <p class="modal-desc">
        {#if udevStatus.needs_relogin}
          Hardware hotkey rules are installed, but your active session is missing <code>input</code> group permissions. Please **log out and log back in** (or reboot) for these settings to take effect.
        {:else}
          VoxCtrl requires global hotkey setup to capture keyboard shortcuts natively. The <code>install.sh</code> script must be run to configure system hardware permissions.
        {/if}
      </p>
      
      <div class="modal-actions">
        {#if !udevStatus.needs_relogin}
          <button
            class="btn-primary"
            onclick={handleDownload}
          >
            📥 Download install.sh
          </button>
        {/if}
        <button class="btn-secondary" onclick={handleClose}>
          Continue Anyway
        </button>
      </div>
    </div>
  {:else}
    <div class="loading-container">
      <div class="spinner"></div>
      <span class="loading-label">Verifying hardware permissions...</span>
    </div>
  {/if}
</div>

<style>
  @reference "tailwindcss";

  .diagnostic-window {
    @apply flex items-center justify-center w-screen h-screen bg-[var(--color-obsidian-950)] text-[var(--text)] overflow-hidden p-5;
  }

  .modal-card {
    @apply text-center max-w-[400px] w-full animate-[scaleUp_0.3s_cubic-bezier(0.175,0.885,0.32,1.2)_forwards];
  }

  .modal-icon {
    @apply text-[48px] mb-4 inline-block drop-shadow-[0_4px_12px_rgba(255,107,53,0.2)] animate-[pulseIcon_2s_infinite_ease-in-out];
  }

  @keyframes pulseIcon {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.08); }
  }

  .modal-title {
    @apply text-2xl font-extrabold text-white mb-3 tracking-tight;
  }

  .modal-desc {
    @apply text-[13.5px] leading-relaxed text-[var(--color-obsidian-300)] mb-6 text-left;
  }

  .modal-desc code {
    @apply bg-white/5 px-1.5 py-0.5 rounded font-mono text-[var(--color-accent-blue)];
  }

  .modal-actions {
    @apply flex flex-col gap-2.5 w-full;
  }

  .btn-primary {
    @apply flex items-center justify-center w-full py-2.5 px-4 rounded-md bg-[var(--color-accent-blue)] text-white font-bold no-underline text-[13px] shadow-[0_4px_14px_rgba(56,189,248,0.3)] transition-all duration-150 ease-out;
  }

  .btn-primary:hover {
    @apply scale-[1.02] -translate-y-[1px] shadow-[0_6px_18px_rgba(56,189,248,0.45)] brightness-[1.05] text-white;
  }

  .btn-primary:active {
    @apply scale-[0.97] translate-y-0;
  }

  .btn-secondary {
    @apply flex items-center justify-center w-full py-2.5 px-4 rounded-md bg-[var(--color-obsidian-800)] text-[var(--color-obsidian-300)] border border-[var(--border)] font-semibold text-[13px] transition-all duration-150 ease-out;
  }

  .btn-secondary:hover {
    @apply bg-[var(--color-obsidian-700)] text-white border-white/10 -translate-y-[1px];
  }

  .btn-secondary:active {
    @apply scale-[0.97] translate-y-0;
  }

  /* Sleek loading state */
  .loading-container {
    @apply flex flex-col items-center gap-4;
  }

  .loading-label {
    @apply text-[13px] text-[var(--color-obsidian-300)] font-medium;
  }

  .spinner {
    @apply w-7 h-7 border-[2.5px] border-white/5 border-t-[var(--color-accent-blue)] rounded-full animate-spin;
  }

  @keyframes scaleUp {
    from { transform: scale(0.95); opacity: 0; }
    to { transform: scale(1); opacity: 1; }
  }
</style>
