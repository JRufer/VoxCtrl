<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { onMount } from "svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() { configDirty.set(true); }

  // ── Piper ──────────────────────────────────────────────────────────────────

  const PIPER_VOICES = [
    "en-us-libritts-high",
    "en-us-amy-low",
    "en-us-kathleen-low",
    "en-gb-southern_english_female-low",
    "en-us-ryan-high",
    "en-us-ryan-medium",
    "en-us-ryan-low",
    "en-us-lessac-medium",
    "en-us-lessac-low",
    "en-us-danny-low",
    "en-gb-alan-low"
  ];

  let downloadedMap = $state<Record<string, boolean>>({});
  let checking = $state(false);
  let downloading = $state(false);
  let testing = $state(false);
  let voiceDirError = $state<string | null>(null);

  async function checkAllVoicesDownloaded() {
    checking = true;
    const newMap: Record<string, boolean> = {};
    for (const v of PIPER_VOICES) {
      try {
        newMap[v] = await invoke<boolean>("check_voice_downloaded", {
          voiceName: v,
          voiceDir: cfg.tts.voice_dir,
        });
      } catch (e) {
        console.error("Failed to check download status for voice " + v, e);
        newMap[v] = false;
      }
    }
    downloadedMap = newMap;
    checking = false;
  }

  async function triggerDownload(voice: string) {
    if (downloading) return;
    downloading = true;
    try {
      await invoke("download_voice", {
        voiceName: voice,
        voiceDir: cfg.tts.voice_dir,
      });
      downloadedMap[voice] = true;
    } catch (e) {
      alert(`Failed to download voice: ${e}`);
    } finally {
      downloading = false;
    }
  }

  async function validateVoiceDir() {
    const path = cfg.tts.voice_dir;
    if (!path) {
      voiceDirError = null;
      return;
    }
    const exists = await invoke<boolean>("check_directory_exists", { path });
    voiceDirError = exists ? null : "This folder does not exist. Please create it first or leave blank for the default location.";
    if (!voiceDirError) {
      await checkAllVoicesDownloaded();
    }
  }

  function onVoiceDirChange() { markDirty(); }

  function onVoiceDirKeydown(e: KeyboardEvent) {
    if (e.key === "Enter") (e.currentTarget as HTMLInputElement).blur();
  }

  async function onVoiceChanged() {
    markDirty();
    const selected = cfg.tts.voice;
    if (!downloadedMap[selected]) {
      await triggerDownload(selected);
    }
  }

  async function testTts() {
    if (testing) return;
    testing = true;
    try {
      await invoke("speak_text", {
        text: "Hi, how can I help you today?",
        voice: cfg.tts.engine === "piper" ? cfg.tts.voice : null,
      });
    } catch (e) {
      alert(`TTS test failed: ${e}`);
    } finally {
      testing = false;
    }
  }

  async function previewVoice() {
    await invoke("speak_text", {
      text: "Hello, this is a voice preview from VoxCtrl.",
      voice: cfg.tts.voice
    });
  }

  // ── Kokoro ─────────────────────────────────────────────────────────────────

  const KOKORO_VOICES = [
    // American Female
    { id: "af_heart",    label: "Heart",    group: "American Female" },
    { id: "af_bella",    label: "Bella",    group: "American Female" },
    { id: "af_sarah",    label: "Sarah",    group: "American Female" },
    { id: "af_nicole",   label: "Nicole",   group: "American Female" },
    { id: "af_sky",      label: "Sky",      group: "American Female" },
    { id: "af_alloy",    label: "Alloy",    group: "American Female" },
    { id: "af_aoede",    label: "Aoede",    group: "American Female" },
    { id: "af_jessica",  label: "Jessica",  group: "American Female" },
    { id: "af_kore",     label: "Kore",     group: "American Female" },
    { id: "af_nova",     label: "Nova",     group: "American Female" },
    { id: "af_river",    label: "River",    group: "American Female" },
    // American Male
    { id: "am_adam",     label: "Adam",     group: "American Male" },
    { id: "am_michael",  label: "Michael",  group: "American Male" },
    { id: "am_puck",     label: "Puck",     group: "American Male" },
    { id: "am_echo",     label: "Echo",     group: "American Male" },
    { id: "am_eric",     label: "Eric",     group: "American Male" },
    { id: "am_fenrir",   label: "Fenrir",   group: "American Male" },
    { id: "am_liam",     label: "Liam",     group: "American Male" },
    { id: "am_onyx",     label: "Onyx",     group: "American Male" },
    { id: "am_santa",    label: "Santa",    group: "American Male" },
    // British Female
    { id: "bf_emma",     label: "Emma",     group: "British Female" },
    { id: "bf_alice",    label: "Alice",    group: "British Female" },
    { id: "bf_isabella", label: "Isabella", group: "British Female" },
    { id: "bf_lily",     label: "Lily",     group: "British Female" },
    // British Male
    { id: "bm_george",   label: "George",   group: "British Male" },
    { id: "bm_lewis",    label: "Lewis",    group: "British Male" },
    { id: "bm_daniel",   label: "Daniel",   group: "British Male" },
    { id: "bm_fable",    label: "Fable",    group: "British Male" },
  ];

  const KOKORO_GROUPS = [...new Set(KOKORO_VOICES.map(v => v.group))];

  let kokoroReady = $state(false);
  let kokoroChecking = $state(false);
  let kokoroDownloading = $state(false);

  async function checkKokoroReady() {
    kokoroChecking = true;
    try {
      kokoroReady = await invoke<boolean>("check_kokoro_ready", {
        quality: cfg.tts.kokoro.quality,
        dataDir: cfg.tts.kokoro.data_dir,
      });
    } catch (e) {
      console.error("check_kokoro_ready:", e);
      kokoroReady = false;
    } finally {
      kokoroChecking = false;
    }
  }

  async function downloadKokoro() {
    if (kokoroDownloading) return;
    kokoroDownloading = true;
    try {
      await invoke("download_kokoro", {
        quality: cfg.tts.kokoro.quality,
        dataDir: cfg.tts.kokoro.data_dir,
      });
      kokoroReady = true;
    } catch (e) {
      alert(`Failed to download Kokoro assets: ${e}`);
    } finally {
      kokoroDownloading = false;
    }
  }

  async function previewKokoro() {
    await invoke("speak_text", {
      text: "Hello, this is Kokoro speaking from VoxCtrl.",
      voice: cfg.tts.kokoro.voice,
    });
  }

  // Re-check Kokoro readiness when quality or data_dir changes
  async function onKokoroQualityChange() {
    markDirty();
    kokoroReady = false;
    await checkKokoroReady();
  }

  onMount(() => {
    checkAllVoicesDownloaded();
    if (cfg.tts.engine === "kokoro") checkKokoroReady();
  });

  // Re-check when switching to Kokoro tab
  $effect(() => {
    if (cfg.tts.engine === "kokoro" && !kokoroReady && !kokoroChecking) {
      checkKokoroReady();
    }
  });
</script>

<section>
  <h2>Text to Speech</h2>

  <div class="field-group">
    <h3>TTS Engine</h3>
    <label class="field">
      <span>Enable TTS</span>
      <input type="checkbox" bind:checked={cfg.tts.enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Engine</span>
      <select bind:value={cfg.tts.engine} onchange={() => { markDirty(); if (cfg.tts.engine === "kokoro") checkKokoroReady(); }}>
        <option value="piper">Piper (neural, high quality)</option>
        <option value="kokoro">Kokoro (neural, natural voices)</option>
        <option value="espeak">eSpeak-NG (lightweight)</option>
      </select>
    </label>
    <div class="row">
      <button class="btn-preview" onclick={testTts} disabled={!cfg.tts.enabled || testing}>
        {testing ? "Speaking..." : "Test TTS"}
      </button>
    </div>
  </div>

  <!-- ── Piper section ──────────────────────────────────────────────────── -->
  {#if cfg.tts.engine === "piper"}
  <div class="field-group">
    <h3>Piper Voice</h3>
    <label class="field">
      <span>Voice</span>
      <select bind:value={cfg.tts.voice} onchange={onVoiceChanged}>
        {#each PIPER_VOICES as v}
          <option value={v}>
            {v}{downloadedMap[v] ? " ✔" : ""}
          </option>
        {/each}
      </select>
    </label>

    <div class="voice-status-container">
      {#if checking}
        <span class="status-checking">⏳ Checking local voice files...</span>
      {:else if downloading}
        <span class="status-downloading">⏳ Downloading {cfg.tts.voice} (model + config)...</span>
      {:else if downloadedMap[cfg.tts.voice]}
        <span class="status-downloaded">✔ Voice downloaded and ready</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class="status-missing">❌ Voice files missing</span>
          <button class="btn-download" onclick={() => triggerDownload(cfg.tts.voice)}>
            📥 Download Voice
          </button>
        </div>
      {/if}
    </div>

    <div class="field">
      <span>Voice directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.voice_dir}
        onchange={onVoiceDirChange}
        onblur={validateVoiceDir}
        onkeydown={onVoiceDirKeydown}
        class:field-input-error={!!voiceDirError}
      />
      {#if voiceDirError}
        <p class="field-error-msg">{voiceDirError}</p>
      {/if}
    </div>
    <p class="hint">Default voice directory: <code>~/.local/share/voxctrl/piper-voices/</code></p>

    <div class="row">
      <button class="btn-preview" onclick={previewVoice} disabled={!cfg.tts.enabled || downloading || !downloadedMap[cfg.tts.voice]}>
        Preview Voice
      </button>
    </div>
  </div>
  {/if}

  <!-- ── Kokoro section ─────────────────────────────────────────────────── -->
  {#if cfg.tts.engine === "kokoro"}
  <div class="field-group">
    <h3>Kokoro Voice</h3>

    <label class="field">
      <span>Voice</span>
      <select bind:value={cfg.tts.kokoro.voice} onchange={markDirty}>
        {#each KOKORO_GROUPS as group}
          <optgroup label={group}>
            {#each KOKORO_VOICES.filter(v => v.group === group) as v}
              <option value={v.id}>{v.label}</option>
            {/each}
          </optgroup>
        {/each}
      </select>
    </label>

    <h3>Quality &amp; Speed</h3>

    <label class="field">
      <span>Model quality</span>
      <select bind:value={cfg.tts.kokoro.quality} onchange={onKokoroQualityChange}>
        <option value="f32">Best – f32 (310 MB, highest quality)</option>
        <option value="fp16">Good – fp16 (169 MB, recommended)</option>
        <option value="int8">Fast – int8 (88 MB, fastest)</option>
      </select>
    </label>

    <label class="field">
      <span>Speed ({cfg.tts.kokoro.speed.toFixed(2)}×)</span>
      <input
        type="range" min="0.5" max="2.0" step="0.05"
        bind:value={cfg.tts.kokoro.speed}
        onchange={markDirty}
        class="range-input"
      />
    </label>

    <label class="field">
      <span>Inference steps</span>
      <input
        type="number" min="1" max="16"
        bind:value={cfg.tts.kokoro.steps}
        onchange={markDirty}
        class="number-input"
      />
    </label>
    <p class="hint">Higher steps = more consistent quality. Default 4 is a good balance.</p>

    <h3>Pre-warm</h3>
    <label class="field">
      <span>Pre-warm model on startup</span>
      <input type="checkbox" bind:checked={cfg.tts.kokoro.prewarm} onchange={markDirty} />
    </label>
    <p class="hint">Runs a silent synthesis at startup so the model is cached when you first speak. Adds ~5 s to startup time.</p>

    <h3>Model Files</h3>

    <div class="voice-status-container">
      {#if kokoroChecking}
        <span class="status-checking">⏳ Checking model files...</span>
      {:else if kokoroDownloading}
        <span class="status-downloading">⏳ Downloading Kokoro model &amp; voices (may take a few minutes)...</span>
      {:else if kokoroReady}
        <span class="status-downloaded">✔ Model and voices downloaded and ready</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class="status-missing">❌ Model files missing</span>
          <button class="btn-download" onclick={downloadKokoro}>
            📥 Download
          </button>
        </div>
      {/if}
    </div>

    <div class="field">
      <span>Data directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.kokoro.data_dir}
        onchange={markDirty}
        onblur={checkKokoroReady}
      />
    </div>
    <p class="hint">Default: <code>~/.local/share/voxctrl/kokoro/</code></p>

    <div class="row">
      <button class="btn-preview" onclick={previewKokoro} disabled={!cfg.tts.enabled || !kokoroReady || kokoroDownloading}>
        Preview Voice
      </button>
    </div>
  </div>
  {/if}

  <div class="field-group">
    <h3>Playback</h3>
    <label class="field">
      <span>Show response overlay</span>
      <input type="checkbox" bind:checked={cfg.tts.response_overlay} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Stop key(s)</span>
      <input type="text"
        value={cfg.tts.stop_key.join(", ")}
        onchange={(e) => {
          cfg.tts.stop_key = (e.target as HTMLInputElement).value.split(",").map(s => s.trim());
          markDirty();
        }}
      />
    </label>
  </div>
</section>

<style>
  @import "./tab.css";
  .row { display: flex; gap: 8px; margin-top: 8px; }
  .btn-preview {
    background: var(--surface2);
    border: 1px solid var(--border);
    color: var(--text);
    border-radius: var(--radius);
    padding: 6px 14px;
    font-size: 12px;
    cursor: pointer;
    transition: all 0.2s ease;
  }
  .btn-preview:hover:not(:disabled) {
    background: var(--border);
    color: var(--accent);
  }
  .btn-preview:disabled { opacity: 0.4; cursor: not-allowed; }

  .voice-status-container {
    display: flex;
    align-items: center;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 10px 14px;
    font-size: 13px;
    min-height: 42px;
  }
  .status-downloaded {
    color: #4caf50;
    font-weight: 600;
  }
  .status-downloading {
    color: var(--accent2);
  }
  .status-checking {
    color: var(--text-muted);
  }
  .status-missing-wrapper {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
  }
  .status-missing {
    color: #e57373;
  }
  .btn-download {
    background: var(--accent);
    border: none;
    color: #fff;
    border-radius: var(--radius);
    padding: 6px 12px;
    font-size: 12px;
    cursor: pointer;
    font-weight: 600;
    transition: background 0.2s;
  }
  .btn-download:hover {
    background: var(--accent2);
  }
  .field-input-error {
    border-color: #ef4444 !important;
  }
  .field-input-error:focus {
    border-color: #ef4444;
    box-shadow: 0 0 0 2px rgba(239, 68, 68, 0.15), inset 0 2px 4px rgba(0, 0, 0, 0.2);
  }
  .field-error-msg {
    margin-top: 0.25rem;
    font-size: 0.875rem;
    line-height: 1.25rem;
    color: #ef4444;
  }
  .range-input {
    width: 100%;
    accent-color: var(--accent);
  }
  .number-input {
    width: 80px;
  }
</style>
