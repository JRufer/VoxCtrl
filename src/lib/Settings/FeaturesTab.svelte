<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  import CustomSelect from "./CustomSelect.svelte";
  import GpuToggle from "./GpuToggle.svelte";
  import DeviceChip from "./DeviceChip.svelte";
  import { gpuLabel } from "./gpu";
  import KeyValueListEditor from "./KeyValueListEditor.svelte";
  import { autoResize } from "../actions";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  if (cfg.engine && !cfg.engine.s1_mini) {
    cfg.engine.s1_mini = { enabled: false, styling: "semi-formal", gpu: true };
  }
  function markDirty() {
    config.set(cfg);
    configDirty.set(true);
  }

  // ── S1-mini Dictation Cleanup ────────────────────────────────────────────
  let s1MiniDownloaded = $state(false);
  let s1MiniChecking = $state(false);
  let s1MiniDownloading = $state(false);
  let s1MiniGpu = $state<string | null>(null);


  function ensureS1MiniConfig() {
    if (!cfg.engine.s1_mini) {
      cfg.engine.s1_mini = {
        enabled: false,
        styling: "semi-formal",
        gpu: true,
      };
    }
  }

  async function checkS1MiniDownloaded() {
    s1MiniChecking = true;
    try {
      s1MiniDownloaded = await invoke<boolean>("check_s1_mini_downloaded");
    } catch (e) {
      console.error("Failed to check S1-mini download status", e);
      s1MiniDownloaded = false;
    } finally {
      s1MiniChecking = false;
    }
  }

  async function triggerS1MiniDownload() {
    if (s1MiniDownloading) return;
    s1MiniDownloading = true;
    try {
      await invoke("download_s1_mini_model");
      s1MiniDownloaded = true;
    } catch (e) {
      alert(`Failed to download S1-mini model: ${e}`);
    } finally {
      s1MiniDownloading = false;
    }
  }

  // Pasting is unavailable on some systems (Linux Mint); the toggle is then
  // disabled and the reason shown.
  let pasteSupport = $state<{ supported: boolean; reason: string | null }>({ supported: true, reason: null });
  invoke<{ supported: boolean; reason: string | null }>("paste_support")
    .then((s) => (pasteSupport = s))
    .catch(() => {});

  async function onS1MiniToggle() {
    ensureS1MiniConfig();
    markDirty();
    if (cfg.engine.s1_mini.enabled && !s1MiniDownloaded) {
      await triggerS1MiniDownload();
    }
  }

  let s1MiniEnabled = $derived(!!cfg.engine.s1_mini?.enabled);

  onMount(async () => {
    ensureS1MiniConfig();
    checkS1MiniDownloaded();
    try {
      const support = await invoke<{ s1_mini_gpu: string | null }>("accelerator_support");
      s1MiniGpu = support.s1_mini_gpu ?? null;
    } catch (e) {
      console.error("Failed to query GPU support", e);
    }
  });


  let customVocabString = $derived(
    cfg.features.custom_vocabulary ? cfg.features.custom_vocabulary.join(", ") : ""
  );

  function onCustomVocabChange(e: Event) {
    const target = e.target as HTMLTextAreaElement;
    cfg.features.custom_vocabulary = target.value
      .split(",")
      .map(w => w.trim())
      .filter(w => w.length > 0);
    markDirty();
  }

</script>

<section>
  <h2>Post-Processing</h2>

  <div class="field-group">
    <div class="field-label-row">
      <h3>S1-mini Dictation Cleanup</h3>
      <DeviceChip
        state={s1MiniDownloaded ? "ready" : s1MiniDownloading ? "downloading" : s1MiniEnabled ? "missing" : "checking"}
        backend={s1MiniGpu}
        on={cfg.engine.s1_mini.gpu}
      />
    </div>

    <label class="field">
      <div class="field-title-col">
        <span>Enable S1-mini dictation cleanup</span>
        <p class="hint">
          Uses Superwhisper's local Qwen-based text normalizer to clean speech-to-text transcripts into natural punctuation, casing, and spoken corrections while strictly preserving voice commands.
        </p>
      </div>
      <input
        type="checkbox"
        bind:checked={cfg.engine.s1_mini.enabled}
        onchange={onS1MiniToggle}
      />
    </label>

    <div class="s1-mini-note">
      Note: S1-Mini is a ~480 MB download.
    </div>

    {#if s1MiniEnabled}
      <GpuToggle
        backend={s1MiniGpu}
        on={cfg.engine.s1_mini.gpu}
        onchange={(v) => {
          cfg.engine.s1_mini.gpu = v;
          markDirty();
        }}
        hint="Off runs S1-mini on the CPU. Takes effect on the next cleanup."
      />
      <div class="model-status-container mt-1">
        {#if s1MiniChecking}
          <span class="status-checking">⏳ Checking S1-mini model files...</span>
        {:else if s1MiniDownloading}
          <span class="status-downloading"
            >⏳ Downloading S1-mini model (s1-mini-q4_k_m.gguf & tokenizer.json)...</span
          >
        {:else if s1MiniDownloaded}
          <span class="status-downloaded">✔ Model downloaded and ready {s1MiniGpu && cfg.engine.s1_mini.gpu ? `(${gpuLabel(s1MiniGpu)} GPU accelerated)` : "on CPU"}</span>
        {:else}
          <div class="status-missing-wrapper">
            <span class="status-missing">❌ Model files missing</span>
            <button
              class="btn-download"
              type="button"
              onclick={triggerS1MiniDownload}
            >
              📥 Download S1-mini
            </button>
          </div>
        {/if}
      </div>

      <label class="field">
        <span>Cleanup styling</span>
        <CustomSelect
          bind:value={cfg.engine.s1_mini.styling}
          options={[
            { value: "semi-formal", label: "Semi-formal (Default)" },
            { value: "casual", label: "Casual" },
            { value: "formal", label: "Formal" },
            { value: "verbatim", label: "Verbatim" },
            { value: "concise", label: "Concise" }
          ]}
          onchange={markDirty}
        />
      </label>
    {/if}
  </div>

  <div class="field-group" class:disabled-section={s1MiniEnabled}>
    <h3>Basic Text Cleanup</h3>
    {#if s1MiniEnabled}
      <p class="hint disabled-note">
        These features are covered by S1-mini when enabled.
      </p>
    {/if}
    <label class="field">
      <span>Remove filler words (uh, um, hmm…)</span>
      <input type="checkbox" bind:checked={cfg.features.remove_fillers} onchange={markDirty} disabled={s1MiniEnabled} />
    </label>
    <label class="field">
      <span>Spoken punctuation ("period" → ".")</span>
      <input type="checkbox" bind:checked={cfg.features.spoken_punctuation} onchange={markDirty} disabled={s1MiniEnabled} />
    </label>
    <label class="field">
      <span>Auto-format lists ("first, second, third")</span>
      <input type="checkbox" bind:checked={cfg.features.auto_format_lists} onchange={markDirty} disabled={s1MiniEnabled} />
    </label>
  </div>

  <div class="field-group">
    <h3>Voice Commands</h3>
    <label class="field">
      <span>Recognise commands while you're still speaking</span>
      <input type="checkbox" bind:checked={cfg.features.early_command_detection} onchange={markDirty} />
    </label>
    <p class="hint">
      Transcribes the first few seconds of each recording early, so a command like
      “Hey Vox, say …” shows its overlay and starts loading the voice before you let go
      of the hotkey. Where the text goes is still decided by the full transcript. Turn
      this off to save the extra transcription work.
    </p>
  </div>

  <div class="field-group">
    <h3>Text Delivery</h3>
    <label class="field">
      <span>Paste text instead of typing it</span>
      {#if pasteSupport.supported}
        <input type="checkbox" bind:checked={cfg.features.paste_instead_of_typing} onchange={markDirty} />
      {:else}
        <input type="checkbox" checked={false} disabled />
      {/if}
    </label>
    {#if !pasteSupport.supported}
      <p class="hint">{pasteSupport.reason}</p>
    {/if}
    <p class="hint">
      Sends each transcription as one paste instead of a key press per character, so
      nothing is typed into an app that has no text field focused. Your whole clipboard
      (text, images, files, rich text — every format) is backed up first and put back
      afterwards, unless you copy something else in the meantime. If a paste cannot be
      sent, VoxCtrl types the text instead. Turn this off to always type.
    </p>
    <label class="field">
      <span>Paste shortcut</span>
      <select bind:value={cfg.features.paste_shortcut} onchange={markDirty} disabled={!pasteSupport.supported || !cfg.features.paste_instead_of_typing}>
        <option value="auto">Automatic (recommended)</option>
        <option value="ctrl+v">Ctrl+V</option>
        <option value="ctrl+shift+v">Ctrl+Shift+V</option>
        <option value="shift+insert">Shift+Insert</option>
      </select>
    </label>
    <p class="hint">
      Automatic uses Ctrl+V, switching to Ctrl+Shift+V in terminal windows and
      Shift+Insert in Windows consoles. Pick one explicitly only if automatic fails
      for a window VoxCtrl cannot identify — for example a native Wayland terminal on
      GNOME.
    </p>
  </div>

  <div class="field-group">
    <h3>Custom Dictionary</h3>
    <p class="hint">Provide a comma-separated list of words (e.g. names or jargon like "Waylin, Rufer, Enola, Kenz") that are hard to spell. The transcription process will correct these in the final text.</p>
    <textarea 
      class="custom-vocab-input"
      placeholder="e.g. Waylin, Rufer, Enola, Kenz"
      value={customVocabString}
      oninput={onCustomVocabChange}
      use:autoResize
    ></textarea>
  </div>

  <KeyValueListEditor
    bind:value={cfg.features.snippets}
    onchange={markDirty}
    title="Snippets"
    hint="Type a trigger word → it expands to the replacement text."
    addLabel="Add Snippet"
    keyPlaceholder="Trigger word"
    valuePlaceholder="Expansion text"
    emptyText="No snippets defined."
  />
</section>

<style lang="postcss">
  @reference "../../app.css";

  .custom-vocab-input {
    @apply w-full min-h-[80px] bg-[var(--bg)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 px-3 text-[13px] resize-y mt-2 outline-none box-border transition-all duration-200 ease-out;
  }

  .custom-vocab-input:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(79,195,247,0.2)];
  }

  .custom-vocab-input::placeholder {
    @apply text-[var(--text-muted)] opacity-50;
  }

  .model-status-container {
    @apply flex items-center bg-[var(--bg)] border border-[var(--border)] rounded-[var(--radius)] p-2.5 px-3.5 text-[13px] min-h-[42px] mb-3;
  }
  .status-downloaded {
    @apply text-emerald-400 font-semibold;
  }
  .status-downloading {
    @apply text-[var(--accent2)];
  }
  .status-checking {
    @apply text-[var(--text-muted)];
  }
  .status-missing-wrapper {
    @apply flex items-center justify-between w-full;
  }
  .status-missing {
    @apply text-red-400;
  }
  .btn-download {
    @apply bg-[var(--accent)] border-none text-white rounded-[var(--radius)] p-1.5 px-3 text-xs cursor-pointer font-semibold transition-colors duration-200;
  }
  .btn-download:hover {
    @apply bg-[var(--accent2)];
  }


  .field-title-col {
    @apply flex flex-col flex-1 mr-4;
  }
  .field-title-col span {
    @apply text-[13px] font-medium text-[var(--color-obsidian-100)];
  }
  .field-title-col .hint {
    @apply text-xs text-[var(--text-muted)] mt-0.5 leading-relaxed;
  }
  .s1-mini-note {
    @apply text-[11.5px] font-medium text-[var(--color-accent-blue)] opacity-90 -mt-1;
  }

  .disabled-section {
    @apply opacity-50 pointer-events-none;
  }
  .disabled-note {
    @apply text-[var(--color-accent-blue)] opacity-90;
  }
</style>
