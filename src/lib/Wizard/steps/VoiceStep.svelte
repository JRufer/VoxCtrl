<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { get } from "svelte/store";
  import { config } from "../../../stores/config";
  import { status } from "../../../stores/status";
  import { patchConfig, wizard } from "../wizard-state.svelte";
  import {
    TTS_ENGINES,
    formatPercent,
    formatSize,
    modelSizeShare,
    ttsSpeedLabel,
    waveBars,
    type TtsEngineId,
  } from "../wizard-data";

  let {
    setBlocker,
  }: {
    setBlocker: (step: number, reason: string | null) => void;
  } = $props();

  const STEP = 5;

  const enabled = $derived($config.tts.enabled);
  const selected = $derived($config.tts.engine as TtsEngineId);
  const selectedEngine = $derived(TTS_ENGINES.find((e) => e.id === selected) ?? TTS_ENGINES[0]);

  /** engine id → its model/voice files are on disk. */
  let ready = $state<Record<string, boolean>>({ espeak: true });
  let checking = $state<Record<string, boolean>>({});
  let downloading = $state<string | null>(null);
  let errors = $state<Record<string, string>>({});

  /** Whether this build compiled the Inflect Micro engine at all. */
  let inflectAvailable = $state(true);

  /**
   * The single HuggingFace access token, shared by every gated model. Without
   * it those engines cannot be downloaded at all.
   */
  let envToken = $state<string | null>(null);
  const fromEnv = $derived(!!envToken);
  const hfToken = $derived(envToken ?? ($config.tts.hf_token ?? "").trim());
  const hasHfToken = $derived(hfToken.length > 0);
  const gatedEngines = TTS_ENGINES.filter((e) => e.needsHfToken);

  function setHfToken(value: string) {
    if (fromEnv) return;
    tokenRejected = false;
    patchConfig((cfg) => {
      cfg.tts.hf_token = value.trim() ? value.trim() : null;
    });
  }

  /**
   * Whether a gated engine is still out of reach.
   */
  function locked(id: TtsEngineId) {
    const engine = TTS_ENGINES.find((e) => e.id === id);
    return !!engine?.needsHfToken && !hasHfToken && !ready[id];
  }

  let tokenRejected = $state(false);

  const HF_TOKEN_REJECTED_TAG = "hf-token-rejected";

  function isTokenRejection(error: unknown): boolean {
    return `${error}`.toLowerCase().includes(HF_TOKEN_REJECTED_TAG);
  }

  let playing = $state<string | null>(null);
  let playError = $state<string | null>(null);
  const bars = waveBars(12);

  let playWatchdog: ReturnType<typeof setTimeout> | null = null;
  let playPoll: ReturnType<typeof setInterval> | null = null;
  let playStartedAt = 0;
  let sawSpeaking = false;

  const PLAY_POLL_MS = 300;
  const PLAY_SETTLE_MS = 1500;
  const PLAY_TIMEOUT_MS = 30_000;

  function clearPlayTimers() {
    if (playWatchdog) {
      clearTimeout(playWatchdog);
      playWatchdog = null;
    }
    if (playPoll) {
      clearInterval(playPoll);
      playPoll = null;
    }
  }

  function stopPlaying() {
    playing = null;
    clearPlayTimers();
  }

  async function pollSpeaking() {
    if (!playing) return;
    let payload: { speaking?: boolean } | undefined;
    try {
      payload = await invoke<{ speaking?: boolean }>("get_status");
    } catch (e) {
      console.error("Wizard: status poll failed:", e);
      return;
    }
    if (payload) status.set(payload as any);

    if (payload?.speaking) {
      sawSpeaking = true;
      return;
    }
    if (sawSpeaking || Date.now() - playStartedAt > PLAY_SETTLE_MS) stopPlaying();
  }

  function setErr(id: string, msg: string | null) {
    const next = { ...errors };
    if (msg) next[id] = msg;
    else delete next[id];
    errors = next;
  }

  /** Ask the backend whether one engine's assets are already present. */
  async function checkEngine(id: TtsEngineId): Promise<boolean> {
    const cfg = get(config);
    try {
      switch (id) {
        case "espeak":
          return true;
        case "piper":
          return await invoke<boolean>("check_voice_downloaded", {
            voiceName: cfg.tts.voice,
            voiceDir: cfg.tts.voice_dir,
          });
        case "pocket_tts":
          return await invoke<boolean>("check_pocket_tts_ready", {
            voice: cfg.tts.pocket_tts.voice,
            voiceDir: cfg.tts.pocket_tts.voice_dir,
          });
        case "inflect_micro":
          return await invoke<boolean>("check_inflect_micro_downloaded", {
            modelDir: cfg.tts.inflect_micro?.model_dir ?? "",
          });
        case "breeze_tts_2":
          return await invoke<boolean>("check_breeze_tts_2_ready", {
            modelDir: cfg.tts.breeze_tts_2?.model_dir ?? "",
          });
        case "vox_cpm_2":
          return await invoke<boolean>("check_vox_cpm_2_ready", {
            modelDir: cfg.tts.vox_cpm_2?.model_dir ?? "",
          });
      }
    } catch (e) {
      console.error("Wizard: TTS readiness check failed for", id, e);
      return false;
    }
  }

  async function refreshAll() {
    await Promise.all(
      TTS_ENGINES.map(async (engine) => {
        checking = { ...checking, [engine.id]: true };
        const ok = await checkEngine(engine.id);
        ready = { ...ready, [engine.id]: ok };
        checking = { ...checking, [engine.id]: false };
      }),
    );
  }

  async function download(id: TtsEngineId) {
    if (downloading) return;
    downloading = id;
    setErr(id, null);
    const cfg = get(config);
    try {
      switch (id) {
        case "piper":
          await invoke("download_voice", {
            voiceName: cfg.tts.voice,
            voiceDir: cfg.tts.voice_dir,
          });
          break;
        case "pocket_tts":
          await invoke("download_pocket_tts", {
            voice: cfg.tts.pocket_tts?.voice,
            voiceDir: cfg.tts.pocket_tts?.voice_dir,
            hfToken: cfg.tts.hf_token,
          });
          break;
        case "inflect_micro":
          await invoke("download_inflect_micro", {
            modelDir: cfg.tts.inflect_micro?.model_dir ?? "",
          });
          break;
        case "breeze_tts_2":
          await invoke("download_breeze_tts_2", {
            modelDir: cfg.tts.breeze_tts_2?.model_dir ?? "",
            hfToken: cfg.tts.hf_token,
          });
          break;
        case "vox_cpm_2":
          await invoke("download_vox_cpm_2", {
            modelDir: cfg.tts.vox_cpm_2?.model_dir ?? "",
            hfToken: cfg.tts.hf_token,
          });
          break;
        case "espeak":
          break;
      }
      tokenRejected = false;
      ready = { ...ready, [id]: true };
      wizard.clearIssue(`tts-download-${id}`);
    } catch (e) {
      const engine = TTS_ENGINES.find((t) => t.id === id);
      const rejected = isTokenRejection(e);
      if (rejected) tokenRejected = true;

      setErr(
        id,
        rejected
          ? `HuggingFace did not accept that access token, so ${engine?.name ?? id} could not be ` +
            `downloaded. Check the token is a valid read token, and that the same account has ` +
            `accepted the licence at ${engine?.licenceUrl ?? "huggingface.co"}.`
          : `${e}`,
      );
      wizard.recordIssue({
        id: `tts-download-${id}`,
        step: STEP,
        title: rejected
          ? `${engine?.name ?? id} could not be downloaded — HuggingFace did not accept the access token.`
          : `${engine?.name ?? id} could not be downloaded — speech output will stay silent.`,
        detail: `engine=${id}\n${e}`,
      });
    } finally {
      downloading = null;
    }
  }

  function pick(id: TtsEngineId) {
    if (locked(id)) return;
    patchConfig((cfg) => {
      cfg.tts.enabled = true;
      cfg.tts.engine = id;
    });
  }

  function setEnabled(on: boolean) {
    patchConfig((cfg) => {
      cfg.tts.enabled = on;
    });
  }

  /**
   * Speak a sample through the engine the user just picked — the same path
   * Settings → TTS uses, so a sample that works here works in the app.
   */
  async function play(id: TtsEngineId) {
    if (playing) {
      stopPlaying();
      void invoke("stop_tts").catch(() => {});
      return;
    }
    pick(id);
    playError = null;
    playing = id;
    playStartedAt = Date.now();
    sawSpeaking = false;
    clearPlayTimers();
    playPoll = setInterval(() => void pollSpeaking(), PLAY_POLL_MS);
    playWatchdog = setTimeout(() => {
      stopPlaying();
      playError =
        "The sample never finished playing. The engine may have failed silently — " +
        "check Settings → TTS, or try another voice.";
    }, PLAY_TIMEOUT_MS);
    const cfg = get(config);
    const engine = TTS_ENGINES.find((e) => e.id === id)!;
    const voice =
      id === "piper" ? cfg.tts.voice : id === "pocket_tts" ? cfg.tts.pocket_tts.voice : null;
    try {
      await invoke("save_config", { newConfig: cfg });
      await invoke("speak_text", { text: `Hi this is ${engine.name} speaking from VoxCtrl`, voice });
    } catch (e) {
      playError = `${e}`;
      stopPlaying();
      wizard.recordIssue({
        id: `tts-speak-${id}`,
        step: STEP,
        title: `${engine.name} failed to speak the sample.`,
        detail: `engine=${id} voice=${voice ?? "(fixed)"}\n${e}`,
      });
    }
  }

  $effect(() => {
    if (!enabled) {
      setBlocker(STEP, null);
      return;
    }
    if (downloading) {
      setBlocker(STEP, `Downloading ${TTS_ENGINES.find((e) => e.id === downloading)?.name}…`);
      return;
    }
    if (locked(selected)) {
      setBlocker(STEP, "Enter a HuggingFace access token, or pick a voice that does not need one.");
      return;
    }
    if (selected === "inflect_micro" && !inflectAvailable) {
      setBlocker(STEP, "This build has no Inflect Micro engine — pick another voice or skip.");
      return;
    }
    setBlocker(
      STEP,
      ready[selected]
        ? null
        : "Download the voice you picked, or turn speech output off to continue.",
    );
  });

  onMount(() => {
    void refreshAll();
    invoke<string | null>("hf_token_env")
      .then((t) => (envToken = t && t.trim() ? t.trim() : null))
      .catch(() => (envToken = null));
    invoke<boolean>("inflect_micro_available")
      .then((v) => (inflectAvailable = v))
      .catch(() => (inflectAvailable = false));

    const subs = [
      listen("tts-playback-end", () => stopPlaying()),
      listen<string>("tts-error", (e) => {
        playError = e.payload;
        stopPlaying();
        wizard.recordIssue({
          id: "tts-runtime",
          step: STEP,
          title: "The text-to-speech engine reported an error while speaking.",
          detail: `engine=${get(config).tts.engine}\n${e.payload}`,
        });
      }),
    ];
    return () => {
      setBlocker(STEP, null);
      clearPlayTimers();
      for (const s of subs) void s.then((off) => off()).catch(() => {});
      void invoke("stop_tts").catch(() => {});
    };
  });

  const isSelectedReady = $derived(!!ready[selected]);
  const isSelectedLocked = $derived(locked(selected));
  const isSelectedUnavailable = $derived(selected === "inflect_micro" && !inflectAvailable);
  const canPlaySelected = $derived(
    isSelectedReady && !isSelectedUnavailable && !isSelectedLocked && !downloading,
  );
</script>

<div class="voice-step">
  <div class="head">
    <div class="copy">
      <span class="vx-eyebrow">// 05 · text to speech · optional</span>
      <h2 class="vx-title">Should VoxCtrl talk back?</h2>
      <p class="vx-lede">
        Agents can stream replies back through a response pipe and VoxCtrl speaks them aloud. Richer
        voices need bigger models and more time per sentence; lighter engines answer instantly.
      </p>
    </div>

    <div class="choice">
      <button class="vx-card mode" class:vx-on={enabled} onclick={() => setEnabled(true)}>
        <span class="mode-glyph on">⊕</span>
        <span>
          <span class="mode-title">Enable speech output</span>
          <span class="mode-desc">Downloads once · runs offline</span>
        </span>
      </button>
      <button class="vx-card mode" class:off-on={!enabled} onclick={() => setEnabled(false)}>
        <span class="mode-glyph">⊘</span>
        <span>
          <span class="mode-title">Skip for now</span>
          <span class="mode-desc">Enable later in Settings → TTS</span>
        </span>
      </button>
    </div>
  </div>

  <!-- Unified Audition & Simplified HuggingFace Token Toolbar -->
  <div class="toolbar">
    <div class="audition-panel">
      <button
        class="vx-btn audition-btn"
        class:playing={!!playing}
        disabled={!canPlaySelected && !playing}
        title={playing
          ? "Stop"
          : isSelectedReady
            ? "Play a sample"
            : isSelectedLocked
              ? "Needs a HuggingFace access token"
              : "Download this voice first"}
        onclick={() => {
          if (playing) {
            stopPlaying();
            void invoke("stop_tts").catch(() => {});
          } else {
            void play(selected);
          }
        }}
      >
        {#if playing}
          <span class="stop-icon">■</span>
          <span class="btn-text">Stop</span>
          <span class="play-bars">
            {#each bars as b}
              <div style:animation-duration="{b.d}s" style:animation-delay="{b.dl}s"></div>
            {/each}
          </span>
        {:else}
          <span class="tri">▶</span>
          <span class="btn-text">play sample</span>
        {/if}
      </button>

      <div class="audition-meta">
        <span class="audition-label">
          Audition: {selectedEngine.name}
        </span>
        <span class="audition-hint">
          {#if playing}
            <span class="good">Speaking sample aloud…</span>
          {:else if downloading}
            <span class="warn">Downloading voice…</span>
          {:else if isSelectedLocked}
            <span class="bad">Locked (token required)</span>
          {:else if isSelectedReady}
            <span class="ready">Ready for playback</span>
          {:else}
            <span class="dim">Download voice first</span>
          {/if}
        </span>
      </div>
    </div>

    <!-- Simplified HuggingFace Token UI -->
    <div
      class="hf-bar"
      class:needed={enabled && (tokenRejected || gatedEngines.some((e) => locked(e.id)))}
    >
      <div class="hf-header">
        <span class="hf-title">HuggingFace access token</span>
        <span class="hf-sub">needed for Breeze-TTS-2 &amp; Pocket TTS</span>
      </div>
      <div class="hf-control">
        <input
          class="hf-input"
          type="password"
          autocomplete="off"
          spellcheck="false"
          placeholder="hf_…"
          readonly={fromEnv}
          title={fromEnv
            ? "Set by the HF_TOKEN environment variable"
            : "Enter a HuggingFace read token for gated models"}
          value={hfToken}
          oninput={(e) => setHfToken((e.currentTarget as HTMLInputElement).value)}
        />
        <span class="hf-state" class:ok={hasHfToken && !tokenRejected} class:bad={tokenRejected}>
          {#if tokenRejected}
            did not accept this token
          {:else if fromEnv}
            HF_TOKEN environment variable
          {:else if hasHfToken}
            ✓ token saved
          {:else}
            token required for gated voices
          {/if}
        </span>
      </div>
    </div>
  </div>

  <!-- 3-Column × 2-Row Responsive Grid -->
  <div class="grid" class:muted={!enabled}>
    {#each TTS_ENGINES as engine}
      {@const on = enabled && selected === engine.id}
      {@const isReady = !!ready[engine.id]}
      {@const busy = downloading === engine.id}
      {@const unavailable = engine.id === "inflect_micro" && !inflectAvailable}
      {@const needsToken = locked(engine.id)}
      <div
        class="vx-card card"
        class:vx-on={on}
        class:locked={needsToken}
        aria-disabled={needsToken}
        role="radio"
        aria-checked={on}
        tabindex="0"
        onclick={() => pick(engine.id)}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            pick(engine.id);
          }
        }}
      >
        <div class="card-head">
          <div class="card-title-group">
            <div class="name">{engine.name}</div>
            <div class="kind">{engine.kind}</div>
          </div>
          <div class="vx-check"><span>{on ? "✓" : ""}</span></div>
        </div>

        <div class="actions">
          {#if engine.mb === 0}
            <div class="bundled">bundled · zero download</div>
          {:else if busy}
            <button class="vx-btn dl" disabled><span class="vx-spinner"></span> Downloading…</button>
          {:else if isReady}
            <div class="bundled ok">✓ Downloaded ({formatSize(engine.mb)})</div>
          {:else}
            <button
              class="vx-btn dl"
              disabled={!!downloading || unavailable || needsToken}
              onclick={(e) => {
                e.stopPropagation();
                void download(engine.id);
              }}
            >
              ↓ Download {formatSize(engine.mb)}
            </button>
          {/if}
        </div>

        <div class="metrics">
          {#each [
            { label: "quality", pct: Math.round(engine.quality * 100), value: formatPercent(engine.quality), color: "var(--vx-cyan-0)" },
            { label: "speed", pct: Math.round(engine.speed * 100), value: ttsSpeedLabel(engine.speed), color: "var(--vx-cyan-2)" },
            { label: "model size", pct: modelSizeShare(engine.mb), value: formatSize(engine.mb), color: "var(--vx-gold-1)" }
          ] as m}
            <div class="metric">
              <div class="metric-head"><span>{m.label}</span><span>{m.value}</span></div>
              <div class="vx-meter"><div style:width="{m.pct}%" style:background={m.color}></div></div>
            </div>
          {/each}
        </div>

        <div class="note">
          {#if needsToken}
            <span class="bad">
              Needs a HuggingFace access token — enter one above to unlock this voice.
            </span>
          {:else if errors[engine.id]}
            <span class="bad">{errors[engine.id]}</span>
          {:else if engine.needsHfToken && !hasHfToken && isReady}
            <span class="ok">
              Already downloaded — the token is only needed to fetch the weights, so this voice
              works without one.
            </span>
          {:else if unavailable}
            <span class="bad">
              This build was compiled without the `inflect-micro` feature, so this engine cannot
              synthesize.
            </span>
          {:else}
            {engine.note}
          {/if}
        </div>
      </div>
    {/each}
  </div>

  {#if playError}
    <div class="play-error">⚠ {playError}</div>
  {/if}
</div>

<style>
  .voice-step {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .head {
    display: flex;
    gap: 20px;
    align-items: flex-end;
    justify-content: space-between;
    flex: none;
    min-width: 0;
  }

  .copy {
    max-width: 680px;
    min-width: 0;
  }

  .copy .vx-lede {
    margin: 4px 0 0;
    font-size: 13px;
    line-height: 1.45;
  }

  .choice {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
    flex: none;
    width: 480px;
  }

  .mode {
    height: 58px;
    padding: 0 14px;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .mode.off-on {
    border-color: var(--vx-line-2);
    background: var(--vx-bg-3);
    box-shadow: 0 0 0 1px var(--vx-line-2);
  }

  .mode-glyph {
    font-family: var(--vx-mono);
    font-size: 20px;
    color: var(--vx-txt-2);
  }

  .mode-glyph.on {
    color: var(--vx-cyan-1);
  }

  .mode-title {
    display: block;
    font-weight: 600;
    font-size: 13.5px;
  }

  .mode-desc {
    display: block;
    font-size: 11.5px;
    color: var(--vx-txt-2);
  }

  /* Toolbar: Audition Controls + Simplified HF Token */
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 8px 14px;
    border: 1px solid var(--vx-line);
    border-radius: 10px;
    background: rgba(255, 255, 255, 0.02);
    flex: none;
    transition: opacity 0.4s, filter 0.4s;
  }

  .audition-panel {
    display: flex;
    align-items: center;
    gap: 14px;
    min-width: 0;
  }

  .audition-btn {
    height: 38px;
    padding: 0 16px;
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 600;
    border-radius: 8px;
    background: rgba(34, 212, 239, 0.09);
    border: 1px solid var(--vx-cyan-0);
    color: var(--vx-cyan-0);
    cursor: pointer;
    transition: all 0.2s;
  }

  .audition-btn:hover:not(:disabled) {
    background: rgba(34, 212, 239, 0.18);
    box-shadow: 0 0 12px rgba(34, 212, 239, 0.25);
  }

  .audition-btn:disabled {
    opacity: 0.4;
    cursor: not-allowed;
    border-color: var(--vx-line-2);
    color: var(--vx-txt-3);
    background: rgba(255, 255, 255, 0.03);
  }

  .audition-btn.playing {
    border-color: var(--vx-gold-1);
    color: var(--vx-gold-1);
    background: rgba(234, 179, 8, 0.12);
  }

  .tri {
    font-family: var(--vx-mono);
    font-size: 14px;
    color: inherit;
  }

  .stop-icon {
    font-size: 14px;
    color: inherit;
  }

  .btn-text {
    font-family: var(--vx-mono);
    font-size: 12px;
    letter-spacing: 0.02em;
  }

  .play-bars {
    display: flex;
    align-items: center;
    gap: 2.5px;
    height: 18px;
    margin-left: 4px;
  }

  .play-bars > div {
    width: 3px;
    height: 18px;
    border-radius: 1.5px;
    background: currentColor;
    transform-origin: center;
    animation-name: vxBar;
    animation-iteration-count: infinite;
    animation-timing-function: ease-in-out;
  }

  .audition-meta {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .audition-label {
    font-size: 12px;
    color: var(--vx-cyan-0);
    font-weight: 600;
  }

  .audition-hint {
    font-size: 11px;
    font-family: var(--vx-mono);
  }

  .audition-hint .ready {
    color: var(--vx-good);
  }

  .audition-hint .good {
    color: var(--vx-cyan-0);
  }

  .audition-hint .warn {
    color: var(--vx-gold-1);
  }

  .audition-hint .bad {
    color: var(--vx-bad);
  }

  .audition-hint .dim {
    color: var(--vx-txt-3);
  }

  /* Compact HuggingFace Token Bar */
  .hf-bar {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 4px 10px;
    border-radius: 8px;
    border: 1px solid transparent;
    background: rgba(0, 0, 0, 0.2);
  }

  .hf-bar.needed {
    border-color: color-mix(in srgb, var(--vx-gold-1) 45%, transparent);
    background: color-mix(in srgb, var(--vx-gold-1) 6%, transparent);
  }

  .hf-header {
    display: flex;
    flex-direction: column;
    gap: 1px;
    text-align: right;
  }

  .hf-title {
    font-size: 11.5px;
    font-weight: 600;
  }

  .hf-sub {
    font-size: 10px;
    color: var(--vx-txt-3);
  }

  .hf-control {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .hf-input {
    width: 170px;
    padding: 5px 8px;
    font-size: 11.5px;
    font-family: inherit;
    color: inherit;
    background: rgba(0, 0, 0, 0.45);
    border: 1px solid var(--vx-line);
    border-radius: 6px;
    outline: none;
    transition: border-color 0.2s;
  }

  .hf-input:focus {
    border-color: var(--vx-cyan-0);
  }

  .hf-input[readonly] {
    color: var(--vx-txt-2);
    cursor: not-allowed;
  }

  .hf-state {
    font-size: 9.5px;
    font-family: var(--vx-mono);
    color: var(--vx-txt-3);
    white-space: nowrap;
  }

  .hf-state.ok {
    color: var(--vx-good);
  }

  .hf-state.bad {
    color: var(--vx-bad);
  }

  /* 3-Column × 2-Row Grid for 6 Engines */
  .grid {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    grid-template-rows: repeat(2, minmax(0, 1fr));
    gap: 10px;
    transition: opacity 0.4s, filter 0.4s;
  }

  .grid.muted {
    opacity: 0.18;
    filter: grayscale(1) blur(1px);
    pointer-events: none;
  }

  .card {
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-height: 0;
    cursor: pointer;
    transition: all 0.2s;
  }

  .card.locked {
    opacity: 0.55;
  }

  .card.locked .name {
    color: var(--vx-txt-2);
  }

  .card-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }

  .card-title-group {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .name {
    font-weight: 600;
    font-size: 14.5px;
    letter-spacing: -0.01em;
    white-space: nowrap;
  }

  .kind {
    font-family: var(--vx-mono);
    font-size: 10px;
    color: var(--vx-txt-2);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .actions {
    display: grid;
  }

  .dl {
    height: 30px;
    width: 100%;
    font-size: 11.5px;
    padding: 0 10px;
  }

  .bundled {
    height: 30px;
    display: flex;
    align-items: center;
    justify-content: center;
    border-radius: 8px;
    border: 1px dashed var(--vx-line);
    font-family: var(--vx-mono);
    font-size: 10.5px;
    color: var(--vx-txt-3);
    text-align: center;
  }

  .bundled.ok {
    border-style: solid;
    border-color: rgba(106, 212, 138, 0.35);
    color: var(--vx-good);
  }

  .metrics {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .metric-head {
    display: flex;
    justify-content: space-between;
    font-family: var(--vx-mono);
    font-size: 10.5px;
    color: var(--vx-txt-2);
    margin-bottom: 4px;
  }

  .metric-head span:last-child {
    color: var(--vx-txt-1);
  }

  .note {
    font-size: 11px;
    color: var(--vx-txt-2);
    line-height: 1.35;
    margin-top: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
  }

  .bad {
    color: var(--vx-bad);
    word-break: break-word;
  }

  .note .ok {
    color: var(--vx-good);
  }

  .play-error {
    flex: none;
    font-size: 12px;
    color: var(--vx-bad);
  }

  @media (max-width: 1100px) {
    .grid {
      grid-template-columns: repeat(2, 1fr);
      grid-template-rows: repeat(3, minmax(0, 1fr));
    }

    .head {
      flex-direction: column;
      align-items: stretch;
    }

    .choice {
      width: 100%;
    }
  }

  @media (max-width: 800px) {
    .toolbar {
      flex-direction: column;
      align-items: stretch;
    }

    .hf-header {
      text-align: left;
    }

    .hf-input {
      width: 100%;
    }
  }
</style>
