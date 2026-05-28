<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

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
          VoxCtr requires global hotkey setup to capture keyboard shortcuts natively. The <code>install.sh</code> script must be run to configure system hardware permissions.
        {/if}
      </p>
      
      <div class="modal-actions">
        {#if !udevStatus.needs_relogin}
          <a
            class="btn-primary"
            href="https://github.com/JRufer/VoxCtr/blob/master/install.sh"
            target="_blank"
            rel="noopener noreferrer"
          >
            📥 Download install.sh
          </a>
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
  .diagnostic-window {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100vw;
    height: 100vh;
    background: var(--color-obsidian-950); /* Force deepest dark background immediately */
    color: var(--text);
    overflow: hidden;
    padding: 20px;
  }

  .modal-card {
    text-align: center;
    max-width: 400px;
    width: 100%;
    animation: scaleUp 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.2) forwards;
  }

  .modal-icon {
    font-size: 48px;
    margin-bottom: 16px;
    display: inline-block;
    filter: drop-shadow(0 4px 12px rgba(255, 107, 53, 0.2));
    animation: pulseIcon 2s infinite ease-in-out;
  }

  @keyframes pulseIcon {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.08); }
  }

  .modal-title {
    font-size: 20px;
    font-weight: 800;
    color: #fff;
    margin-bottom: 12px;
    letter-spacing: -0.5px;
  }

  .modal-desc {
    font-size: 13.5px;
    line-height: 1.6;
    color: var(--color-obsidian-300);
    margin-bottom: 24px;
    text-align: left;
  }

  .modal-desc code {
    background: rgba(255, 255, 255, 0.05);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: monospace;
    color: var(--color-accent-blue);
  }

  .modal-actions {
    display: flex;
    flex-direction: column;
    gap: 10px;
    width: 100%;
  }

  .btn-primary {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    padding: 10px 16px;
    border-radius: 6px;
    background: var(--color-accent-blue);
    color: #fff;
    font-weight: 750;
    text-decoration: none;
    font-size: 13px;
    box-shadow: 0 4px 14px rgba(56, 189, 248, 0.3);
    transition: var(--transition-snappy-fast);
  }

  .btn-primary:hover {
    transform: scale(1.02) translateY(-1px);
    box-shadow: 0 6px 18px rgba(56, 189, 248, 0.45);
    filter: brightness(1.05);
    color: #fff;
  }

  .btn-primary:active {
    transform: scale(0.97) translateY(0px);
  }

  .btn-secondary {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100%;
    padding: 10px 16px;
    border-radius: 6px;
    background: var(--color-obsidian-800);
    color: var(--color-obsidian-300);
    border: 1px solid var(--border);
    font-weight: 600;
    font-size: 13px;
    transition: var(--transition-snappy-fast);
  }

  .btn-secondary:hover {
    background: var(--color-obsidian-700);
    color: #fff;
    border-color: rgba(255, 255, 255, 0.1);
    transform: translateY(-1px);
  }

  .btn-secondary:active {
    transform: scale(0.97) translateY(0px);
  }

  /* Sleek loading state */
  .loading-container {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 16px;
  }

  .loading-label {
    font-size: 13px;
    color: var(--color-obsidian-300);
    font-weight: 500;
  }

  .spinner {
    width: 28px;
    height: 28px;
    border: 2.5px solid rgba(255, 255, 255, 0.04);
    border-top: 2.5px solid var(--color-accent-blue);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    0% { transform: rotate(0deg); }
    100% { transform: rotate(360deg); }
  }

  @keyframes scaleUp {
    from { transform: scale(0.95); opacity: 0; }
    to { transform: scale(1); opacity: 1; }
  }
</style>
