<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import CustomSelect from "./CustomSelect.svelte";
  import { fallbackOptions } from "./remote-fallback";
  import {
    connectionSummary,
    ensureServerConfig,
    normalizeServerAddress,
    serverUses,
    setCleanupOnServer,
    setRecognitionOnServer,
    setSpeechOnServer,
    type ServerReport,
  } from "./server-setup";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();

  ensureServerConfig(cfg);

  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  // The fields and switches read from local state, so they follow a change
  // at once whether or not the config object passed in is reactive.
  let address = $state(cfg.engine.remote_openai.endpoint ?? "");
  let apiKey = $state(cfg.engine.remote_openai.api_key ?? "");
  let uses = $state(serverUses(cfg));
  let fallback = $state(cfg.engine.remote_openai.fallback ?? "none");

  // What to go back to when a switch is turned off again in this window.
  const backendBefore = cfg.engine.backend === "remote-openai" ? null : cfg.engine.backend;
  const ttsEngineBefore = cfg.tts.engine === "server" ? null : cfg.tts.engine;

  let testing = $state(false);
  let report = $state<ServerReport | null>(null);
  let summary = $derived(report ? connectionSummary(report) : null);
  let hasAddress = $derived(address.trim().length > 0);

  let moonshine = $state(true);
  let parakeet = $state(true);
  let fallbackChoices = $derived(fallbackOptions(cfg.engine, { moonshine, parakeet }));

  $effect(() => {
    invoke<boolean>("moonshine_available").then((ok) => (moonshine = !!ok)).catch(() => {});
    invoke<boolean>("parakeet_available").then((ok) => (parakeet = !!ok)).catch(() => {});
  });

  function saveAddress() {
    address = normalizeServerAddress(address);
    cfg.engine.remote_openai.endpoint = address;
    cfg.engine.remote_openai.api_key = apiKey.trim() ? apiKey.trim() : null;
    markDirty();
  }

  async function connect() {
    if (testing) return;
    saveAddress();
    if (!address) return;
    testing = true;
    report = null;
    const remote = cfg.engine.remote_openai;
    const args = { endpoint: remote.endpoint, apiKey: remote.api_key || null };
    try {
      const [stt, cleanup, speech] = await Promise.all([
        invoke<{ success: boolean; message: string; models: string[] }>("test_remote_stt", {
          ...args,
          model: remote.model || "whisper-1",
          timeoutSecs: remote.timeout_secs || 30,
        }),
        invoke<{ available: boolean; message: string }>("check_server_cleanup", args),
        invoke<{ available: boolean; message: string }>("check_server_tts", args),
      ]);
      report = {
        recognition: { ok: stt.success, message: stt.message, models: stt.models ?? [] },
        cleanup,
        speech,
      };
      // A VoxCtrl server serves one speech model: use the name it reports.
      if (stt.success && stt.models?.length && !stt.models.includes(remote.model)) {
        remote.model = stt.models[0];
        markDirty();
      }
    } catch (e) {
      report = {
        recognition: { ok: false, message: String(e), models: [] },
        cleanup: { available: false, message: "" },
        speech: { available: false, message: "" },
      };
    } finally {
      testing = false;
    }
  }

  // Switches set the config from the event: a change handler runs before a
  // binding would have written the new value.
  function checked(e: Event) {
    return (e.currentTarget as HTMLInputElement).checked;
  }

  function onRecognition(e: Event) {
    setRecognitionOnServer(cfg, checked(e), backendBefore);
    uses = serverUses(cfg);
    markDirty();
  }

  function onCleanup(e: Event) {
    setCleanupOnServer(cfg, checked(e));
    uses = serverUses(cfg);
    markDirty();
  }

  function onSpeech(e: Event) {
    setSpeechOnServer(cfg, checked(e), ttsEngineBefore);
    uses = serverUses(cfg);
    markDirty();
  }

  function onFallback() {
    cfg.engine.remote_openai.fallback = fallback;
    markDirty();
  }
</script>

<div class="field-group" data-testid="server-setup">
  <h3>VoxCtrl Server</h3>
  <p class="hint">
    Hand speech recognition, dictation cleanup and text-to-speech to a VoxCtrl server on your network, so
    this computer runs no models for them. Enter the address and API key the server printed when it first
    started, connect, then choose what to use it for.
  </p>

  <label class="field">
    <span>Server address</span>
    <input
      type="text"
      bind:value={address}
      onchange={saveAddress}
      placeholder="http://192.168.1.10:8787/v1"
      spellcheck="false"
      autocomplete="off"
      data-testid="server-address"
    />
  </label>
  <label class="field">
    <span>API key</span>
    <input
      type="password"
      bind:value={apiKey}
      onchange={saveAddress}
      placeholder="vx_…"
      autocomplete="off"
      data-testid="server-key"
    />
  </label>
  <div class="connect-row">
    <p class="hint">Just the machine's name or IP address is enough; the rest is filled in.</p>
    <button class="btn-action" onclick={connect} disabled={testing || !hasAddress} data-testid="server-connect">
      {testing ? "Connecting…" : "Connect"}
    </button>
  </div>

  {#if summary}
    <p class="result" class:ok={summary.ok} class:bad={!summary.ok} data-testid="server-summary">
      {summary.ok ? "✔" : "⚠️"} {summary.text}
    </p>
  {/if}

  <h4>Use the server for</h4>

  <label class="field">
    <div class="title-col">
      <span>Speech recognition</span>
      <p class="hint">
        Your dictation is transcribed on the server. Turning this off goes back to a speech engine on this
        computer.
      </p>
    </div>
    <input
      type="checkbox"
      checked={uses.recognition}
      onchange={onRecognition}
      disabled={!hasAddress}
      data-testid="server-use-recognition"
    />
  </label>

  {#if uses.recognition}
    <label class="field">
      <span>If the server can't be reached</span>
      <CustomSelect bind:value={fallback} options={fallbackChoices} onchange={onFallback} />
    </label>
    <p class="hint under">
      The engine on this computer that takes over. It is loaded only while the server is unreachable. With
      none, dictation fails with an error until the server is back.
    </p>
  {/if}

  <label class="field">
    <div class="title-col">
      <span>Dictation cleanup</span>
      <p class="hint">
        {#if !uses.recognition}
          Needs speech recognition on the server.
        {:else if report && !report.cleanup.available}
          {report.cleanup.message}
        {:else}
          S1-mini cleanup runs on the server. Whether and how a dictation is cleaned is still set under
          Post-Processing.
        {/if}
      </p>
    </div>
    <input
      type="checkbox"
      checked={uses.cleanup}
      onchange={onCleanup}
      disabled={!uses.recognition}
      data-testid="server-use-cleanup"
    />
  </label>

  <label class="field">
    <div class="title-col">
      <span>Text-to-speech</span>
      <p class="hint">
        {#if report && !report.speech.available && report.recognition.ok}
          {report.speech.message}
        {:else}
          The server speaks and this computer plays the audio. Choose its engine and voice, including your
          own voices, under TTS.
        {/if}
      </p>
    </div>
    <input
      type="checkbox"
      checked={uses.speech}
      onchange={onSpeech}
      disabled={!hasAddress}
      data-testid="server-use-speech"
    />
  </label>

  <p class="hint">
    These are the same settings the Engine, Post-Processing and TTS tabs show, gathered in one place. Setting
    up the server itself is covered in the server guide (docs/server.md).
  </p>
</div>

<style lang="postcss">
  @reference "../../app.css";

  h4 {
    @apply m-0 mt-3 mb-1 text-[11px] font-bold uppercase text-[var(--accent2)];
  }
  .title-col {
    @apply flex flex-col flex-1 mr-4;
  }
  .title-col .hint {
    @apply mt-0.5;
  }
  .connect-row {
    @apply flex items-center justify-between gap-4 my-1;
  }
  .connect-row .hint {
    @apply m-0;
  }
  .hint.under {
    @apply mt-0 mb-2;
  }
  .result {
    @apply text-xs px-3 py-1.5 rounded-[var(--radius)] border my-1;
  }
  .result.ok {
    @apply bg-emerald-500/15 text-emerald-300 border-emerald-500/30;
  }
  .result.bad {
    @apply bg-red-500/15 text-red-300 border-red-500/30;
  }
</style>
