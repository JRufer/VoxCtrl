<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();
  function markDirty() { configDirty.set(true); }

  // ── Run Speed Timer ────────────────────────────────────────────────────────
  let runSpeed = $state<number | null>(null);
  let elapsed = $state(0);
  let isCounting = $state(false);
  let timerId: any = null;
  let unlistenTtsStart: (() => void) | null = null;
  let unlistenTtsEnd: (() => void) | null = null;
  let voiceSpeaking = $state(false);
  let startTime = 0;

  function startTimer() {
    isCounting = true;
    elapsed = 0;
    runSpeed = null;
    startTime = performance.now();
    
    if (timerId) clearInterval(timerId);
    timerId = setInterval(() => {
      elapsed = Math.round(performance.now() - startTime);
      if (elapsed > 10000) {
        clearInterval(timerId);
        isCounting = false;
        runSpeed = null;
      }
    }, 10);
  }

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
  }

  async function testTts() {
    if (testing) return;
    
    if (voiceSpeaking) {
      voiceSpeaking = false;
      try {
        await invoke("stop_tts");
      } catch (err) {
        console.error("Failed to stop TTS:", err);
      }
    }
    
    testing = true;
    startTimer();
    
    let engineName = "TTS";
    let voice: string | null = null;
    
    if (cfg.tts.engine === "piper") {
      engineName = "Piper";
      voice = cfg.tts.voice;
    } else if (cfg.tts.engine === "kokoro") {
      engineName = "Kokoro";
      voice = cfg.tts.kokoro.voice;
    } else if (cfg.tts.engine === "espeak") {
      engineName = "eSpeak-NG";
      voice = null;
    }
    
    const textToSpeak = `Hi this is ${engineName} speaking from VoxCtrl`;
    
    try {
      await invoke("speak_text", {
        text: textToSpeak,
        voice: voice,
      });
    } catch (e) {
      alert(`TTS test failed: ${e}`);
      clearInterval(timerId);
      isCounting = false;
      testing = false;
    }
  }

  let engineSwitching = $state(false);

  function isTestTtsDisabled() {
    if (!cfg.tts.enabled || engineSwitching) return true;
    if (voiceSpeaking) return false;
    if (testing) return true;
    
    if (cfg.tts.engine === "piper") {
      return checking || downloading || !downloadedMap[cfg.tts.voice];
    }
    if (cfg.tts.engine === "kokoro") {
      return kokoroChecking || kokoroDownloading || !kokoroReady;
    }
    return false;
  }

  // ── Kokoro ─────────────────────────────────────────────────────────────────

  const KOKORO_VOICES = [
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
    { id: "am_adam",     label: "Adam",     group: "American Male" },
    { id: "am_michael",  label: "Michael",  group: "American Male" },
    { id: "am_puck",     label: "Puck",     group: "American Male" },
    { id: "am_echo",     label: "Echo",     group: "American Male" },
    { id: "am_eric",     label: "Eric",     group: "American Male" },
    { id: "am_fenrir",   label: "Fenrir",   group: "American Male" },
    { id: "am_liam",     label: "Liam",     group: "American Male" },
    { id: "am_onyx",     label: "Onyx",     group: "American Male" },
    { id: "am_santa",    label: "Santa",    group: "American Male" },
    { id: "bf_emma",     label: "Emma",     group: "British Female" },
    { id: "bf_alice",    label: "Alice",    group: "British Female" },
    { id: "bf_isabella", label: "Isabella", group: "British Female" },
    { id: "bf_lily",     label: "Lily",     group: "British Female" },
    { id: "bm_george",   label: "George",   group: "British Male" },
    { id: "bm_lewis",    label: "Lewis",    group: "British Male" },
    { id: "bm_daniel",   label: "Daniel",   group: "British Male" },
    { id: "bm_fable",    label: "Fable",    group: "British Male" },
  ];

  const KOKORO_GROUPS = [...new Set(KOKORO_VOICES.map(v => v.group))];

  let kokoroReady = $state(false);
  let kokoroChecking = $state(false);
  let kokoroDownloading = $state(false);
  let kokoroDirError = $state<string | null>(null);

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

  function onKokoroDirChange() { markDirty(); }

  async function validateKokoroDir() {
    const path = cfg.tts.kokoro.data_dir;
    if (!path) {
      kokoroDirError = null;
      return;
    }
    const exists = await invoke<boolean>("check_directory_exists", { path });
    kokoroDirError = exists ? null : "This folder does not exist. Please create it first or leave blank for the default location.";
    if (!kokoroDirError) {
      await checkKokoroReady();
    }
  }

  async function onEngineChanged() {
    markDirty();
    engineSwitching = true;
    try {
      if (cfg.tts.engine === "kokoro") {
        kokoroReady = false;
        if (cfg.tts.kokoro.data_dir) {
          await validateKokoroDir();
        } else {
          await checkKokoroReady();
        }
      } else if (cfg.tts.engine === "piper") {
        if (cfg.tts.voice_dir) {
          await validateVoiceDir();
        } else {
          await checkAllVoicesDownloaded();
        }
      }
    } finally {
      // Add a small 400ms delay to allow the backend save_config to run
      setTimeout(() => {
        engineSwitching = false;
      }, 400);
    }
  }

  onMount(async () => {
    if (cfg.tts.voice_dir) {
      validateVoiceDir();
    } else {
      checkAllVoicesDownloaded();
    }

    if (cfg.tts.kokoro.data_dir) {
      validateKokoroDir();
    } else if (cfg.tts.engine === "kokoro") {
      checkKokoroReady();
    }

    unlistenTtsStart = await listen<void>("tts-playback-start", () => {
      if (isCounting) {
        clearInterval(timerId);
        runSpeed = elapsed;
        isCounting = false;
      }
      testing = false;
      voiceSpeaking = true;
    });

    unlistenTtsEnd = await listen<void>("tts-playback-end", () => {
      voiceSpeaking = false;
    });
  });

  onDestroy(() => {
    if (timerId) clearInterval(timerId);
    if (unlistenTtsStart) unlistenTtsStart();
    if (unlistenTtsEnd) unlistenTtsEnd();
  });

  // ── Stop Key Recorder ───────────────────────────────────────────────────────────

  let isRecordingStopKey = $state(false);
  let currentlyPressedStopKeys = $state<string[]>([]);

  function mapBrowserKeyToEvdev(key: string, code: string): string {
    const codeUpper = code.toUpperCase();
    if (key === "Control") return "KEY_LEFTCTRL";
    if (key === "Alt") return "KEY_LEFTALT";
    if (key === "Shift") return "KEY_LEFTSHIFT";
    if (key === "Meta" || key === "OS" || key === "Super") return "KEY_LEFTMETA";
    if (codeUpper === "SPACE") return "KEY_SPACE";
    if (codeUpper === "ENTER") return "KEY_ENTER";
    if (codeUpper === "ESCAPE" || codeUpper === "ESC") return "KEY_ESC";
    if (codeUpper === "TAB") return "KEY_TAB";
    if (codeUpper === "BACKSPACE") return "KEY_BACKSPACE";
    if (codeUpper === "DELETE") return "KEY_DELETE";
    if (codeUpper.startsWith("KEY")) return codeUpper;
    if (codeUpper.startsWith("DIGIT")) return `KEY_${codeUpper.replace("DIGIT", "")}`;
    if (codeUpper.startsWith("ARROW")) return `KEY_${codeUpper.replace("ARROW", "")}`;
    if (codeUpper.startsWith("F") && codeUpper.length > 1) return `KEY_${codeUpper}`;
    if (key.length === 1) return `KEY_${key.toUpperCase()}`;
    return `KEY_${codeUpper}`;
  }

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
      cfg.tts.stop_key = [...currentlyPressedStopKeys];
      markDirty();
      currentlyPressedStopKeys = [];
      isRecordingStopKey = false;
    }
  }

  function handleStopKeyUp(e: KeyboardEvent) {
    if (!isRecordingStopKey) return;
    e.preventDefault();
    e.stopPropagation();
    if (currentlyPressedStopKeys.length > 0) {
      cfg.tts.stop_key = [...currentlyPressedStopKeys];
      markDirty();
    }
    currentlyPressedStopKeys = [];
    isRecordingStopKey = false;
  }

  function handleStopKeyBlur() {
    // Safety net: if blur fires while we have pending keys (e.g. Escape blur race),
    // commit whatever was captured rather than discarding it silently.
    if (currentlyPressedStopKeys.length > 0) {
      cfg.tts.stop_key = [...currentlyPressedStopKeys];
      markDirty();
      currentlyPressedStopKeys = [];
    }
    isRecordingStopKey = false;
  }

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
      <select bind:value={cfg.tts.engine} onchange={onEngineChanged}>
        <option value="kokoro">Kokoro (neural, natural voices)</option>
        <option value="piper">Piper (neural, high quality)</option>
        <option value="espeak">eSpeak-NG (lightweight)</option>
      </select>
    </label>
    <label class="field">
      <span>Speed ({cfg.tts.speed.toFixed(2)}×)</span>
      <input
        type="range" min="0.5" max="2.0" step="0.05"
        bind:value={cfg.tts.speed}
        onchange={markDirty}
        class="range-input"
      />
    </label>
    <div class="row tts-test-row">
      <button class="btn-preview" onclick={testTts} disabled={isTestTtsDisabled()}>
        {testing ? "Speaking..." : voiceSpeaking ? "⏹ Stop & Test" : "Test TTS"}
      </button>
      {#if isCounting || runSpeed !== null}
        <div class="run-speed-container">
          <span class="run-speed-label">Run speed</span>
          <span class="run-speed-value" class:counting={isCounting}>
            {isCounting ? `${elapsed} ms` : `${runSpeed} ms`}
          </span>
        </div>
      {/if}
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
      {:else}
        <div class="status-missing-wrapper">
          <span class={downloadedMap[cfg.tts.voice] ? "status-downloaded" : "status-missing"}>
            {downloadedMap[cfg.tts.voice] ? "✔ Voice downloaded and ready" : "❌ Voice files missing"}
          </span>
          <button class="btn-download" onclick={() => triggerDownload(cfg.tts.voice)} disabled={downloadedMap[cfg.tts.voice] || downloading}>
            {downloadedMap[cfg.tts.voice] ? "Downloaded" : "📥 Download"}
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

    <div class="voice-status-container">
      {#if kokoroChecking}
        <span class="status-checking">⏳ Checking local model files...</span>
      {:else if kokoroDownloading}
        <span class="status-downloading">⏳ Downloading Kokoro model &amp; voices (may take a few minutes)...</span>
      {:else}
        <div class="status-missing-wrapper">
          <span class={kokoroReady ? "status-downloaded" : "status-missing"}>
            {kokoroReady ? "✔ Model and voices downloaded and ready" : "❌ Model files missing"}
          </span>
          <button class="btn-download" onclick={downloadKokoro} disabled={kokoroReady || kokoroDownloading}>
            {kokoroReady ? "Downloaded" : "📥 Download"}
          </button>
        </div>
      {/if}
    </div>

    <div class="field">
      <span>Voice directory (leave blank for default)</span>
      <input
        type="text"
        bind:value={cfg.tts.kokoro.data_dir}
        onchange={onKokoroDirChange}
        onblur={validateKokoroDir}
        onkeydown={onVoiceDirKeydown}
        class:field-input-error={!!kokoroDirError}
      />
      {#if kokoroDirError}
        <p class="field-error-msg">{kokoroDirError}</p>
      {/if}
    </div>
    <p class="hint">Default voice directory: <code>~/.local/share/voxctrl/kokoro/</code></p>
  </div>
  {/if}

  <div class="field-group">
    <h3>Playback</h3>
    <label class="field">
      <span>Show response overlay</span>
      <input type="checkbox" bind:checked={cfg.tts.response_overlay} onchange={markDirty} />
    </label>

    <div class="border-t border-white/5 pt-[14px] flex flex-col gap-2">
      <h5 class="mb-1 text-[11px] font-bold uppercase text-accent-blue tracking-[0.06em]">Stop Key Bind</h5>
      <p class="hint" style="margin: 0 0 8px 0;">Press a key combo to immediately stop TTS playback — works even when this window is hidden.</p>
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
            {#if cfg.tts.stop_key.length > 0}
              <div class="flex gap-1.5">
                {#each cfg.tts.stop_key as k}
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
    </div>
  </div>
</section>

<style>
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
  .btn-download:hover:not(:disabled) {
    background: var(--accent2);
  }
  .btn-download:disabled {
    background: var(--surface2);
    border: 1px solid var(--border);
    color: var(--text-muted);
    opacity: 0.5;
    cursor: not-allowed;
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

  .tts-test-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
  }
  .run-speed-container {
    display: flex;
    align-items: center;
    gap: 8px;
    background: rgba(255, 255, 255, 0.02);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 12px;
    font-size: 12px;
    font-weight: 500;
  }
  .run-speed-label {
    color: var(--text-muted);
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .run-speed-value {
    color: var(--accent);
    font-family: 'JetBrains Mono', monospace;
    font-weight: 600;
  }
  .run-speed-value.counting {
    color: var(--accent2);
    text-shadow: 0 0 8px rgba(56, 189, 248, 0.3);
  }

</style>
