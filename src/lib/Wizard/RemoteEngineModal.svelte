<script lang="ts">
  import { untrack } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { config } from "../../stores/config";
  import { patchConfig } from "./wizard-state.svelte";

  interface RemoteSttTestResult {
    success: boolean;
    message: string;
    models?: string[];
  }

  let {
    isOpen = $bindable(false),
    onSuccess,
  }: {
    isOpen: boolean;
    onSuccess?: () => void;
  } = $props();

  let endpoint = $state("");
  let apiKey = $state("");
  let model = $state("whisper-1");
  let language = $state("");
  let timeoutSecs = $state(30);

  let remoteTesting = $state(false);
  let remoteTestStatus = $state<{ success: boolean; message: string } | null>(null);
  let remoteDiscoveredModels = $state<string[]>([]);
  let hasTestedSuccess = $state(false);

  let wasOpen = false;
  // Sync state from config only when modal opens
  $effect(() => {
    if (isOpen && !wasOpen) {
      untrack(() => {
        const cur = $config.engine.remote_openai;
        endpoint = cur?.endpoint || "http://localhost:8000/v1";
        apiKey = cur?.api_key || "";
        model = cur?.model || "whisper-1";
        language = cur?.language || "";
        timeoutSecs = cur?.timeout_secs ?? 30;
        remoteTestStatus = null;
      });
    }
    wasOpen = isOpen;
  });

  function saveConfig() {
    patchConfig((cfg) => {
      if (!cfg.engine.remote_openai) {
        cfg.engine.remote_openai = {
          endpoint: endpoint.trim() || "http://localhost:8000/v1",
          api_key: apiKey.trim() ? apiKey.trim() : null,
          model: model.trim() || "whisper-1",
          language: language.trim(),
          timeout_secs: Number(timeoutSecs) || 30,
        };
      } else {
        cfg.engine.remote_openai.endpoint = endpoint.trim() || "http://localhost:8000/v1";
        cfg.engine.remote_openai.api_key = apiKey.trim() ? apiKey.trim() : null;
        cfg.engine.remote_openai.model = model.trim() || "whisper-1";
        cfg.engine.remote_openai.language = language.trim();
        cfg.engine.remote_openai.timeout_secs = Number(timeoutSecs) || 30;
      }
    });
  }

  async function testRemoteConnection() {
    if (remoteTesting) return;
    saveConfig();
    remoteTesting = true;
    remoteTestStatus = null;
    try {
      const res = await invoke<RemoteSttTestResult>("test_remote_stt", {
        endpoint: endpoint.trim() || "http://localhost:8000/v1",
        apiKey: apiKey.trim() ? apiKey.trim() : null,
        model: model.trim() || "whisper-1",
        timeoutSecs: Number(timeoutSecs) || 30,
      });

      remoteTestStatus = { success: res.success, message: res.message };
      if (res.models && res.models.length > 0) {
        remoteDiscoveredModels = res.models;
        if (!model) {
          model = res.models[0];
          saveConfig();
        }
      }

      if (res.success) {
        hasTestedSuccess = true;
        saveConfig();
        onSuccess?.();
      }
    } catch (e: any) {
      remoteTestStatus = { success: false, message: e.toString() };
    } finally {
      remoteTesting = false;
    }
  }

  function handleClose() {
    saveConfig();
    isOpen = false;
  }

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      handleClose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if isOpen}
  <div
    class="modal-backdrop"
    role="dialog"
    aria-modal="true"
    tabindex="-1"
    onclick={(e) => {
      if (e.target === e.currentTarget) handleClose();
    }}
    onkeydown={(e) => {
      if (e.key === "Escape") handleClose();
    }}
  >
    <div class="modal-card">
      <div class="modal-header">
        <div class="modal-title-row">
          <span class="modal-glyph">🌐</span>
          <h3>Remote Speech Engine Settings</h3>
        </div>
        <button type="button" class="close-btn" onclick={handleClose} aria-label="Close modal">
          ✕
        </button>
      </div>

      <div class="modal-body">
        <p class="modal-hint">
          Connect to a network speech-to-text service supporting the OpenAI
          <code>/v1/audio/transcriptions</code> API (e.g., Faster-Whisper-Server, vLLM, LocalAI, Whisper-standalone, or OpenAI Whisper).
          Audio will begin streaming to the endpoint as soon as your keybind is triggered.
        </p>

        <div class="field-group">
          <label class="field">
            <span class="field-title">Endpoint URL</span>
            <input
              type="text"
              class="vx-input"
              bind:value={endpoint}
              placeholder="http://192.168.1.50:8000/v1"
              onchange={saveConfig}
            />
          </label>
          <p class="field-hint">
            Base URL or full transcriptions path (e.g. <code>http://192.168.1.50:8000/v1</code> or <code>https://api.openai.com/v1</code>).
          </p>

          <label class="field">
            <span class="field-title">API Key (optional)</span>
            <input
              type="password"
              class="vx-input"
              bind:value={apiKey}
              placeholder="Bearer token or leave blank"
              onchange={saveConfig}
            />
          </label>
          <p class="field-hint">
            Leave blank if your local network server does not require authentication.
          </p>

          <label class="field">
            <span class="field-title">Model</span>
            <input
              type="text"
              class="vx-input"
              bind:value={model}
              placeholder="whisper-1"
              onchange={saveConfig}
            />
          </label>

          {#if remoteDiscoveredModels.length > 0}
            <div class="discovered-models">
              <span class="field-hint">Discovered models from server:</span>
              <div class="model-tags">
                {#each remoteDiscoveredModels as m}
                  <button
                    type="button"
                    class="tag-btn"
                    class:active={model === m}
                    onclick={() => {
                      model = m;
                      saveConfig();
                    }}
                  >
                    {m}
                  </button>
                {/each}
              </div>
            </div>
          {/if}

          <label class="field">
            <span class="field-title">Language (optional)</span>
            <input
              type="text"
              class="vx-input"
              bind:value={language}
              placeholder="auto"
              onchange={saveConfig}
            />
          </label>
          <p class="field-hint">
            Language code (e.g. <code>en</code>, <code>es</code>, <code>fr</code>) or leave blank/auto for automatic detection.
          </p>

          <label class="field">
            <span class="field-title">Timeout (seconds)</span>
            <input
              type="number"
              class="vx-input"
              min="5"
              max="300"
              bind:value={timeoutSecs}
              onchange={saveConfig}
            />
          </label>

          <div class="test-row">
            <button
              type="button"
              class="btn-test"
              onclick={testRemoteConnection}
              disabled={remoteTesting}
            >
              {#if remoteTesting}
                ⏳ Testing Connection...
              {:else}
                🧪 Test Connection
              {/if}
            </button>

            {#if remoteTestStatus}
              <div
                class="status-pill"
                class:success={remoteTestStatus.success}
                class:error={!remoteTestStatus.success}
              >
                {#if remoteTestStatus.success}
                  ✔ {remoteTestStatus.message}
                {:else}
                  ⚠️ {remoteTestStatus.message}
                {/if}
              </div>
            {/if}
          </div>
        </div>
      </div>

      <div class="modal-footer">
        <div class="footer-status">
          {#if hasTestedSuccess}
            <span class="status-msg ok">✔ Tested & verified successfully</span>
          {:else}
            <span class="status-msg warn">⚠️ Test connection required to proceed</span>
          {/if}
        </div>
        <div class="footer-actions">
          <button
            type="button"
            class="vx-btn vx-ghost"
            onclick={handleClose}
          >
            Cancel
          </button>
          <button
            type="button"
            class="vx-btn vx-primary"
            onclick={handleClose}
          >
            {hasTestedSuccess ? "Done" : "Save & Close"}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}

<style>
  .modal-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.72);
    backdrop-filter: blur(6px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1200;
    padding: 20px;
    animation: fadeIn 0.2s ease-out;
  }

  @keyframes fadeIn {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }

  .modal-card {
    width: 100%;
    max-width: 580px;
    max-height: 90vh;
    background: var(--vx-bg-1);
    border: 1px solid var(--vx-line-2);
    border-radius: 18px;
    box-shadow: 0 24px 60px rgba(0, 0, 0, 0.7), 0 0 0 1px rgba(34, 212, 239, 0.2);
    display: flex;
    flex-direction: column;
    overflow: hidden;
    color: var(--vx-txt-0);
  }

  .modal-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 16px 22px;
    border-bottom: 1px solid var(--vx-line);
    background: var(--vx-bg-2);
  }

  .modal-title-row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .modal-glyph {
    font-size: 20px;
  }

  .modal-header h3 {
    margin: 0;
    font-size: 16px;
    font-weight: 600;
    letter-spacing: -0.01em;
    color: var(--vx-txt-0);
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--vx-txt-2);
    font-size: 16px;
    cursor: pointer;
    padding: 6px;
    border-radius: 8px;
    transition: all 0.2s;
    line-height: 1;
  }

  .close-btn:hover {
    color: var(--vx-txt-0);
    background: rgba(255, 255, 255, 0.08);
  }

  .modal-body {
    padding: 20px 22px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .modal-hint {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.5;
    color: var(--vx-txt-2);
  }

  .modal-hint code,
  .field-hint code {
    background: var(--vx-bg-4);
    color: var(--vx-cyan-1);
    padding: 2px 5px;
    border-radius: 4px;
    font-family: var(--vx-mono);
    font-size: 11px;
    border: 1px solid var(--vx-line);
  }

  .field-group {
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--vx-line);
    border-radius: 14px;
    padding: 16px 18px;
  }

  .field {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .field-title {
    font-size: 12.5px;
    font-weight: 500;
    color: var(--vx-txt-1);
  }

  .vx-input {
    width: 100%;
    height: 38px;
    padding: 0 12px;
    background: var(--vx-bg-0);
    border: 1px solid var(--vx-line-2);
    border-radius: 8px;
    color: var(--vx-txt-0);
    font-family: inherit;
    font-size: 13px;
    outline: none;
    transition: border-color 0.2s, box-shadow 0.2s;
    box-sizing: border-box;
  }

  .vx-input:focus {
    border-color: var(--vx-cyan-0);
    box-shadow: 0 0 0 2px var(--vx-cyan-b);
  }

  .vx-input::placeholder {
    color: var(--vx-txt-3);
  }

  .field-hint {
    margin: -4px 0 4px 0;
    font-size: 11px;
    line-height: 1.45;
    color: var(--vx-txt-3);
  }

  .discovered-models {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin-top: -4px;
  }

  .model-tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .tag-btn {
    font-size: 11.5px;
    padding: 3px 9px;
    border-radius: 6px;
    background: var(--vx-bg-3);
    border: 1px solid var(--vx-line-2);
    color: var(--vx-txt-2);
    cursor: pointer;
    transition: all 0.2s;
  }

  .tag-btn:hover {
    color: var(--vx-txt-0);
    border-color: var(--vx-cyan-0);
  }

  .tag-btn.active {
    background: rgba(34, 212, 239, 0.15);
    border-color: var(--vx-cyan-0);
    color: var(--vx-cyan-1);
    font-weight: 600;
  }

  .test-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-top: 8px;
    flex-wrap: wrap;
  }

  .btn-test {
    height: 36px;
    padding: 0 16px;
    border-radius: 8px;
    background: var(--vx-bg-4);
    border: 1px solid var(--vx-line-2);
    color: var(--vx-txt-0);
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.2s;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }

  .btn-test:hover:not(:disabled) {
    background: var(--vx-line-2);
    border-color: var(--vx-cyan-0);
  }

  .btn-test:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .status-pill {
    padding: 6px 12px;
    border-radius: 8px;
    font-size: 11.5px;
    font-weight: 500;
    line-height: 1.35;
    word-break: break-word;
    max-width: 100%;
  }

  .status-pill.success {
    background: rgba(106, 212, 138, 0.15);
    color: var(--vx-good);
    border: 1px solid rgba(106, 212, 138, 0.3);
  }

  .status-pill.error {
    background: rgba(244, 99, 110, 0.15);
    color: var(--vx-bad);
    border: 1px solid rgba(244, 99, 110, 0.3);
  }

  .modal-footer {
    padding: 14px 22px;
    border-top: 1px solid var(--vx-line);
    background: var(--vx-bg-2);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .footer-status {
    font-size: 12px;
  }

  .status-msg.ok {
    color: var(--vx-good);
    font-weight: 500;
  }

  .status-msg.warn {
    color: var(--vx-warn);
  }

  .footer-actions {
    display: flex;
    gap: 10px;
  }
</style>
