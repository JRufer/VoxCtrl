<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();

  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  /**
   * The single HuggingFace access token used by every gated model VoxCtrl can
   * download (Pocket-TTS, Breeze-TTS-2, VoxCPM2). Entering it here — or during
   * the setup wizard — writes the same `tts.hf_token` field, so the two stay
   * in sync automatically. A token exported as `HF_TOKEN` wins at download
   * time, so when the session has one it is shown here read-only and never
   * written to the config.
   */
  let envHfToken = $state<string | null>(null);
  const hfFromEnv = $derived(!!envHfToken);
  const hfTokenShown = $derived(envHfToken ?? cfg.tts.hf_token ?? "");

  function onHfTokenChanged(e: Event) {
    if (hfFromEnv) return;
    const val = (e.target as HTMLInputElement).value;
    cfg.tts.hf_token = val.trim() ? val.trim() : null;
    markDirty();
  }

  let wizardError = $state<string | null>(null);

  type UpdateInfo = {
    version: string;
    current_version: string;
    can_self_update: boolean;
  };
  type UpdateCheckPayload = {
    current_version: string;
    update: UpdateInfo | null;
  };

  // Null until the backend answers; false on a package-manager or development
  // install, where automatic checks never run.
  let autoSupported = $state<boolean | null>(null);

  function setCheckAutomatically(on: boolean) {
    // Older configs may predate the section; the backend fills it on load,
    // but a config handed in from elsewhere might not have it yet.
    cfg.updates = { check_automatically: on, skipped_version: cfg.updates?.skipped_version ?? null };
    markDirty();
  }

  let checking = $state(false);
  let checkResult = $state<UpdateCheckPayload | null>(null);
  let checkError = $state<string | null>(null);

  async function checkForUpdate() {
    checking = true;
    checkError = null;
    checkResult = null;
    try {
      checkResult = await invoke<UpdateCheckPayload>("check_for_update");
      // A newer release opens the update window straight away: change log from
      // the user's version to the latest, plus the download.
      if (checkResult.update) await invoke("open_update_window");
    } catch (e) {
      checkError = `${e}`;
    } finally {
      checking = false;
    }
  }

  async function runSetupWizard() {
    wizardError = null;
    try {
      await invoke("open_setup_wizard");
    } catch (e) {
      wizardError = `${e}`;
    }
  }

  onMount(() => {
    invoke<boolean>("automatic_update_checks_supported")
      .then((v) => (autoSupported = v === true))
      .catch(() => (autoSupported = false));
    invoke<string | null>("hf_token_env")
      .then((t) => (envHfToken = t && t.trim() ? t.trim() : null))
      .catch(() => (envHfToken = null));
  });
</script>

<section>
  <h2>General</h2>

  <div class="field-group">
    <h3>Setup</h3>
    <div class="field">
      <span>Run the first-launch setup wizard again</span>
      <button class="btn-action" onclick={runSetupWizard}>Open setup wizard</button>
    </div>
    <p class="hint">
      Walks through the engine, model, hotkey, overlay and voice choices in one flow, and downloads
      whatever it needs. Your current settings stay in place until you change them in the wizard.
    </p>
    {#if wizardError}
      <p class="hint error">Could not open the wizard: {wizardError}</p>
    {/if}
  </div>

  <div class="field-group">
    <h3>Hugging Face Access Token</h3>
    <div class="field">
      <span>Hugging Face access token</span>
      <input
        type="password"
        value={hfTokenShown}
        readonly={hfFromEnv}
        title={hfFromEnv ? "Set by the HF_TOKEN environment variable" : undefined}
        oninput={onHfTokenChanged}
      />
    </div>
    {#if hfFromEnv}
      <p class="hint">
        Using the <code>HF_TOKEN</code> environment variable. It takes precedence over a saved
        token and is not written to your config.
      </p>
    {/if}
    <p class="hint">
      Used by every gated model VoxCtrl can download — Pocket-TTS, Breeze-TTS-2, and VoxCPM2 in
      the TTS tab. Create a token at <code>huggingface.co/settings/tokens</code> and accept each
      model's license on its Hugging Face page before downloading. Enter it once here; the setup
      wizard reads and writes the same token, so the two always match.
    </p>
  </div>


  <div class="field-group">
    <div class="field-label-row">
      <h3>Updates</h3>
      {#if checkResult && !checkResult.update}
        <span class="status-pill success" title="VoxCtrl {checkResult.current_version} is the latest release">✔ Up to date</span>
      {/if}
    </div>
    <p class="hint">
      Press "Check for updates" to ask GitHub whether a newer release has been published — that
      request carries nothing about you or your machine. If there is one, you'll see what changed
      since your version and can download it.
    </p>
    {#if autoSupported === false}
      <p class="hint">
        Automatic checks are not available here: this copy of VoxCtrl was installed by a package
        manager, or is a development build, so updates come the way it was installed.
      </p>
    {:else}
      <label class="field">
        <span>Check for updates automatically</span>
        <input
          type="checkbox"
          checked={cfg.updates?.check_automatically ?? false}
          disabled={autoSupported === null}
          onchange={(e) => setCheckAutomatically((e.currentTarget as HTMLInputElement).checked)}
        />
      </label>
      <p class="hint">
        Off unless you turn it on. When on, VoxCtrl asks GitHub about a minute after it starts and
        then once a day. A new release shows up in the tray menu with one notification, never while
        you are dictating, and nothing is installed until you choose to.
      </p>
    {/if}
    <div class="field">
      <span>Look for a newer VoxCtrl release</span>
      <button class="btn-action" onclick={checkForUpdate} disabled={checking}>
        {checking ? "Checking…" : "Check for updates"}
      </button>
    </div>
    {#if checkError}
      <p class="hint error">Could not check for updates: {checkError}</p>
    {/if}
  </div>

  <div class="field-group">
    <h3>MCP Server</h3>
    <label class="field">
      <span>Enable MCP JSON-RPC server</span>
      <input type="checkbox" bind:checked={cfg.mcp.server_enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Visual Feedback</span>
      <input type="checkbox" bind:checked={cfg.mcp.visual_feedback} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Record timeout (seconds)</span>
      <input
        type="number"
        min="1"
        max="120"
        bind:value={cfg.mcp.record_timeout}
        onchange={markDirty}
      />
    </label>
    <p class="hint">
      How long <code>transcribe_voice</code> listens when the calling agent does not ask for a
      specific timeout. An explicit <code>timeout_seconds</code> in the tool call still wins.
    </p>
    <p class="hint">Socket: <code>$XDG_RUNTIME_DIR/voxctrl-mcp.sock</code>, also reachable as <code>/tmp/voxctrl-mcp.sock</code> (Linux) / <code>\\.\pipe\voxctrl-mcp</code> (Windows)</p>
  </div>
</section>

<style lang="postcss">
  @reference "../../app.css";

  .status-pill {
    @apply text-xs px-3 py-1.5 rounded-[var(--radius)] font-medium leading-normal;
  }
  .status-pill.success {
    @apply bg-emerald-500/15 text-emerald-300 border border-emerald-500/30;
  }

  .hint.error {
    @apply text-red-400;
  }

  .link {
    @apply text-[var(--color-accent-blue)] underline underline-offset-2 bg-transparent border-0 p-0 cursor-pointer text-inherit;
  }
</style>
