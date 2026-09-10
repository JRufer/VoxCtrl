<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { config } from "../../../stores/config";
  import { patchConfig, wizard } from "../wizard-state.svelte";
  import RemoteEngineModal from "../RemoteEngineModal.svelte";
  import {
    STT_ENGINES,
    accuracyBars,
    formatPercent,
    formatSize,
    speedLabel,
    type ModelOption,
    type SttEngineId,
  } from "../wizard-data";

  let {
    registerGate,
    setBlocker,
  }: {
    registerGate: (step: number, gate: (() => Promise<boolean>) | null) => void;
    setBlocker: (step: number, reason: string | null) => void;
  } = $props();

  const STEP = 1;

  /** Whether this build actually contains the Moonshine backend. Without it a
   *  "moonshine" selection silently runs whisper.cpp, which the user deserves
   *  to know before they pick it. */
  let moonshineAvailable = $state(true);
  let parakeetAvailable = $state(true);
  /** The GPU backend whisper.cpp was compiled against, or null on a CPU-only
   *  build. Null is the honest default: a build that cannot answer cannot
   *  offload either, and naming a backend it does not have is how the toggle
   *  came to read "ON · Vulkan" on builds with no GPU support at all. */
  let whisperGpu = $state<string | null>(null);

  /** model id → on disk, per engine. */
  let whisperDownloaded = $state<Record<string, boolean>>({});
  let moonshineDownloaded = $state<Record<string, boolean>>({});
  let parakeetDownloaded = $state<Record<string, boolean>>({});

  let downloading = $state<string | null>(null);
  let downloadError = $state<string | null>(null);
  /** False until the first on-disk probe finishes. Both maps start empty, so
   *  deciding anything before then would report every model as missing. */
  let readinessChecked = $state(false);

  let showRemoteModal = $state(false);
  let remoteTestedSuccessfully = $state(false);

  const selectedEngine = $derived<SttEngineId>(
    $config.engine.backend === "remote-openai"
      ? "remote-openai"
      : $config.engine.backend === "parakeet"
      ? "parakeet"
      : $config.engine.backend === "moonshine"
      ? "moonshine"
      : "whisper-cpp",
  );
  const GPU_LABELS: Record<string, string> = {
    cuda: "CUDA",
    vulkan: "Vulkan",
    coreml: "CoreML",
  };
  /** Offloading is only really on when the build has somewhere to offload to. */
  const gpuOn = $derived(
    !!whisperGpu && $config.engine.whisper_cpp.device !== "cpu",
  );
  const gpuPath = $derived(whisperGpu ? (GPU_LABELS[whisperGpu] ?? whisperGpu) : "");

  /** The model the current engine will actually load. */
  const selectedModel = $derived(
    selectedEngine === "remote-openai"
      ? ($config.engine.remote_openai?.model || "whisper-1")
      : selectedEngine === "parakeet" && parakeetAvailable
      ? $config.engine.parakeet.model_size
      : selectedEngine === "moonshine" && moonshineAvailable
      ? $config.engine.moonshine.model_size
      : $config.engine.whisper_cpp.model_size,
  );

  const selectedReady = $derived(
    selectedEngine === "remote-openai"
      ? remoteTestedSuccessfully
      : selectedEngine === "parakeet" && parakeetAvailable
      ? !!parakeetDownloaded[selectedModel]
      : selectedEngine === "moonshine" && moonshineAvailable
      ? !!moonshineDownloaded[selectedModel]
      : !!whisperDownloaded[selectedModel],
  );

  function isSelected(engine: SttEngineId, model: ModelOption): boolean {
    if (engine !== selectedEngine) return false;
    return model.id === (engine === "remote-openai"
      ? ($config.engine.remote_openai?.model || "whisper-1")
      : engine === "parakeet"
      ? $config.engine.parakeet.model_size
      : engine === "moonshine"
      ? $config.engine.moonshine.model_size
      : $config.engine.whisper_cpp.model_size);
  }

  function downloadedFor(engine: SttEngineId, model: ModelOption): boolean {
    if (engine === "remote-openai") return remoteTestedSuccessfully;
    if (engine === "parakeet") return !!parakeetDownloaded[model.id];
    return engine === "moonshine" ? !!moonshineDownloaded[model.id] : !!whisperDownloaded[model.id];
  }

  function pickEngine(id: SttEngineId) {
    patchConfig((cfg) => {
      cfg.engine.backend = id;
      if (id === "remote-openai" && !cfg.engine.remote_openai) {
        cfg.engine.remote_openai = {
          endpoint: "http://localhost:8000/v1",
          api_key: null,
          model: "whisper-1",
          language: "",
          timeout_secs: 30,
        };
      }
    });
    wizard.engineChosen = true;
    if (id === "remote-openai") {
      wizard.modelChosen = true;
      showRemoteModal = true;
    }
  }

  function pickModel(engine: SttEngineId, model: ModelOption) {
    patchConfig((cfg) => {
      cfg.engine.backend = engine;
      if (engine === "parakeet") cfg.engine.parakeet.model_size = model.id;
      else if (engine === "moonshine") cfg.engine.moonshine.model_size = model.id;
      else if (engine === "remote-openai") {
        if (!cfg.engine.remote_openai) {
          cfg.engine.remote_openai = {
            endpoint: "http://localhost:8000/v1",
            api_key: null,
            model: model.id,
            language: "",
            timeout_secs: 30,
          };
        } else {
          cfg.engine.remote_openai.model = model.id;
        }
      } else cfg.engine.whisper_cpp.model_size = model.id;
    });
    downloadError = null;
    wizard.engineChosen = true;
    wizard.modelChosen = true;
  }

  function toggleGpu() {
    // Nothing to switch on in a CPU-only build: the setting would flip and the
    // engine would go on running exactly as it was.
    if (!whisperGpu) return;
    patchConfig((cfg) => {
      // "auto" means "use the backend this build has", which is a better answer
      // than pinning a name from here — there is only ever one to pick.
      cfg.engine.whisper_cpp.device = gpuOn ? "cpu" : "auto";
    });
  }

  async function refreshWhisper() {
    const dir = $config.engine.whisper_cpp.model_dir;
    const next: Record<string, boolean> = {};
    for (const m of STT_ENGINES[0].models) {
      try {
        next[m.id] = await invoke<boolean>("check_model_downloaded", {
          modelSize: m.id,
          modelDir: dir,
        });
      } catch (e) {
        console.error("Wizard: whisper model check failed for", m.id, e);
        next[m.id] = false;
      }
    }
    whisperDownloaded = next;
  }

  async function refreshMoonshine() {
    const next: Record<string, boolean> = {};
    const moonshineEngine = STT_ENGINES.find((e) => e.id === "moonshine");
    if (moonshineEngine) {
      for (const m of moonshineEngine.models) {
        try {
          next[m.id] = await invoke<boolean>("check_moonshine_downloaded", { modelSize: m.id });
        } catch (e) {
          console.error("Wizard: moonshine model check failed for", m.id, e);
          next[m.id] = false;
        }
      }
    }
    moonshineDownloaded = next;
  }

  async function refreshParakeet() {
    const next: Record<string, boolean> = {};
    const parakeetEngine = STT_ENGINES.find((e) => e.id === "parakeet");
    if (parakeetEngine) {
      for (const m of parakeetEngine.models) {
        try {
          next[m.id] = await invoke<boolean>("check_parakeet_downloaded", { modelSize: m.id });
        } catch (e) {
          console.error("Wizard: parakeet model check failed for", m.id, e);
          next[m.id] = false;
        }
      }
    }
    parakeetDownloaded = next;
  }

  /**
   * Fetch whichever model the user just chose, and only then let the wizard
   * move on. Downloading here rather than in the background is the point of
   * the step: the next screen asks them to press a hotkey and speak, and that
   * cannot work without weights on disk.
   */
  async function ensureModel(): Promise<boolean> {
    if (selectedEngine === "remote-openai") {
      if (!remoteTestedSuccessfully) {
        setBlocker(STEP, "Test the remote connection successfully to continue.");
        showRemoteModal = true;
        return false;
      }
      return true;
    }
    if (selectedReady) return true;
    const engine = selectedEngine;
    const model = selectedModel;
    downloading = model;
    downloadError = null;
    try {
      if (engine === "parakeet" && parakeetAvailable) {
        await invoke("download_parakeet_model", { modelSize: model });
        parakeetDownloaded = { ...parakeetDownloaded, [model]: true };
      } else if (engine === "moonshine" && moonshineAvailable) {
        await invoke("download_moonshine_model", { modelSize: model });
        moonshineDownloaded = { ...moonshineDownloaded, [model]: true };
      } else {
        await invoke("download_model", {
          modelSize: model,
          modelDir: $config.engine.whisper_cpp.model_dir,
        });
        whisperDownloaded = { ...whisperDownloaded, [model]: true };
      }
      wizard.clearIssue("model-download");
      return true;
    } catch (e) {
      downloadError = `${e}`;
      // Logged for the final screen: a missing model is the difference between
      // a working install and a hotkey that records and then produces nothing.
      wizard.recordIssue({
        id: "model-download",
        step: STEP,
        title: `Speech model "${model}" could not be downloaded — dictation will produce no text.`,
        detail: `engine=${engine} model=${model} model_dir=${$config.engine.whisper_cpp.model_dir || "(default)"}\n${e}`,
      });
      return false;
    } finally {
      downloading = null;
    }
  }

  // Continue stays live while a model is merely missing — pressing it is what
  // starts the download — and is blocked while one is in flight.
  //
  // The "you must choose" gate exists for exactly one reason: the config ships
  // with a backend and a size already set, so an untouched screen and a
  // deliberately-kept default look identical, and the cost of guessing wrong is
  // a multi-gigabyte download of a model nobody asked for. When the selected
  // model is already on disk that cost is zero, so there is nothing left to
  // protect the user from and the gate would just be a click they have to
  // perform on their own existing choice.
  $effect(() => {
    if (downloading) {
      setBlocker(STEP, `Downloading ${downloading}…`);
    } else if (!readinessChecked) {
      setBlocker(STEP, "Checking which models are already on disk…");
    } else if (selectedEngine === "remote-openai" && !remoteTestedSuccessfully) {
      setBlocker(STEP, "Test the remote connection successfully to continue.");
    } else if (selectedReady) {
      setBlocker(STEP, null);
    } else if (!wizard.engineChosen) {
      setBlocker(STEP, "Choose a transcription engine to continue.");
    } else if (!wizard.modelChosen) {
      setBlocker(STEP, "Choose a model size to continue.");
    } else {
      setBlocker(STEP, null);
    }
  });

  async function refreshReadiness() {
    readinessChecked = false;
    await Promise.all([refreshWhisper(), refreshMoonshine(), refreshParakeet()]);
    readinessChecked = true;
  }

  onMount(() => {
    registerGate(STEP, ensureModel);
    invoke<boolean>("moonshine_available")
      .then((v) => (moonshineAvailable = v))
      .catch(() => (moonshineAvailable = false));
    invoke<boolean>("parakeet_available")
      .then((v) => (parakeetAvailable = v))
      .catch(() => (parakeetAvailable = false));
    invoke<{ whisper_gpu: string | null }>("accelerator_support")
      .then((v) => (whisperGpu = v?.whisper_gpu ?? null))
      .catch(() => (whisperGpu = null));
    void refreshReadiness();
    return () => {
      registerGate(STEP, null);
      setBlocker(STEP, null);
    };
  });

  /** Metrics for one engine card, recomputed from the model it has selected. */
  function metricsFor(engine: (typeof STT_ENGINES)[number]) {
    if (engine.id === "remote-openai") {
      const remoteModel = $config.engine.remote_openai?.model || "whisper-1";
      return {
        model: { id: remoteModel, mb: 0, speed: 0.95, accuracy: 0.95 },
        rows: [
          { label: "speed", pct: 95, value: "network", color: "var(--vx-cyan-0)", dim: false },
          { label: "accuracy", pct: 95, value: "server", color: "var(--vx-cyan-2)", dim: false },
          { label: "RAM", pct: 2, value: "offloaded", color: "var(--vx-gold-1)", dim: false },
          {
            label: "VRAM",
            pct: 0,
            value: "0 MB",
            color: "var(--vx-gold-0)",
            dim: true,
          },
        ],
        quiet: accuracyBars(12, 0.95),
        noisy: accuracyBars(12, 0.92),
        quietPct: "server",
        noisyPct: "server",
      };
    }
    const chosen =
      engine.id === "parakeet"
        ? $config.engine.parakeet.model_size
        : engine.id === "moonshine"
        ? $config.engine.moonshine.model_size
        : $config.engine.whisper_cpp.model_size;
    const model = engine.models.find((m) => m.id === chosen) ?? engine.models[0];
    const gpu = engine.gpu && gpuOn;
    const speed = Math.min(1, model.speed + (gpu ? 0.3 : 0));
    const ram = gpu ? model.mb * 0.3 : model.mb;
    const vram = gpu ? model.mb * 1.1 : 0;
    return {
      model,
      rows: [
        { label: "speed", pct: Math.round(speed * 100), value: speedLabel(speed), color: "var(--vx-cyan-0)", dim: false },
        { label: "accuracy", pct: Math.round(model.accuracy * 100), value: formatPercent(model.accuracy), color: "var(--vx-cyan-2)", dim: false },
        { label: "RAM", pct: Math.max(2, Math.min(100, Math.round((ram / 3400) * 100))), value: formatSize(ram), color: "var(--vx-gold-1)", dim: false },
        {
          label: "VRAM",
          pct: vram ? Math.max(2, Math.min(100, Math.round((vram / 3400) * 100))) : 0,
          value: vram ? formatSize(vram) : engine.gpu ? "off" : "cpu only",
          color: "var(--vx-gold-0)",
          dim: !vram,
        },
      ],
      quiet: accuracyBars(12, model.accuracy),
      noisy: accuracyBars(12, model.accuracy * engine.noiseRetention),
      quietPct: formatPercent(model.accuracy),
      noisyPct: formatPercent(model.accuracy * engine.noiseRetention),
    };
  }
</script>

<div class="engine-step">
  <div class="head">
    <div class="copy">
      <span class="vx-eyebrow">// 01 · transcription engine</span>
      <h2 class="vx-title">Which ears should VoxCtrl use?</h2>
      <p class="vx-lede">
        Choose between on-device engines (<b>whisper.cpp</b>, <b>Moonshine</b>, <b>Parakeet TDT</b>) or connect to a <b>Remote Speech Engine</b> over your network.
        Pick a model size inside each card or configure your remote endpoint to get started.
      </p>
    </div>

    <button
      class="vx-card gpu-toggle"
      class:vx-on={gpuOn}
      onclick={toggleGpu}
      disabled={!whisperGpu}
    >
      <span class="switch" class:on={gpuOn}><span class="knob"></span></span>
      <span class="gpu-copy">
        <span class="gpu-title">
          <span class="glyph">∗</span> GPU offloading
          <span class="state" class:on={gpuOn}>
            {#if !whisperGpu}
              UNAVAILABLE · CPU BUILD
            {:else if gpuOn}
              ON · {gpuPath}
            {:else}
              OFF · CPU
            {/if}
          </span>
        </span>
        <span class="gpu-desc">
          {#if whisperGpu}
            Moves whisper.cpp weights from RAM to your GPU: faster, less RAM, uses VRAM. Moonshine is
            CPU-native and unaffected.
          {:else}
            This build has no GPU backend compiled in, so both engines run on the CPU. The Vulkan
            download accelerates whisper.cpp; Moonshine is CPU-native either way.
          {/if}
        </span>
      </span>
    </button>
  </div>

  <div class="engines">
    {#each STT_ENGINES as engine}
      {@const m = metricsFor(engine)}
      {@const on = selectedEngine === engine.id}
      <div
        class="vx-card engine-card"
        class:vx-on={on}
        role="radio"
        aria-checked={on}
        tabindex="0"
        onclick={() => {
          pickEngine(engine.id);
          if (engine.id === "remote-openai") {
            showRemoteModal = true;
          }
        }}
        onkeydown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            pickEngine(engine.id);
            if (engine.id === "remote-openai") {
              showRemoteModal = true;
            }
          }
        }}
      >
        <div class="vx-check corner"><span>✓</span></div>

        <div class="engine-head">
          <div class="name-row">
            <span class="engine-glyph">{engine.glyph}</span>
            <span class="engine-name">{engine.name}</span>
          </div>
          <div class="tagline">{engine.tagline}</div>
          {#if engine.id === "moonshine" && !moonshineAvailable}
            <div class="warn">
              This build was compiled without the Moonshine backend — choosing it runs whisper.cpp
              with the model above instead.
            </div>
          {:else if engine.id === "parakeet" && !parakeetAvailable}
            <div class="warn">
              This build was compiled without the Parakeet backend — choosing it runs whisper.cpp
              with the model above instead.
            </div>
          {/if}
        </div>

        <div class="left-col">
          {#if engine.id === "remote-openai"}
            <div class="vx-label">server configuration</div>
            <div class="remote-summary-box">
              <div class="remote-row">
                <span class="remote-k">URL</span>
                <span class="remote-v" title={$config.engine.remote_openai?.endpoint || "http://localhost:8000/v1"}>
                  {$config.engine.remote_openai?.endpoint || "http://localhost:8000/v1"}
                </span>
              </div>
              <div class="remote-row">
                <span class="remote-k">Model</span>
                <span class="remote-v">{$config.engine.remote_openai?.model || "whisper-1"}</span>
              </div>
              <div class="remote-row status">
                <span class="remote-k">Status</span>
                <span class="remote-v" class:remote-v-ok={remoteTestedSuccessfully} class:remote-v-warn={!remoteTestedSuccessfully}>
                  {#if remoteTestedSuccessfully}
                    ✔ Verified
                  {:else}
                    ⚠️ Untested
                  {/if}
                </span>
              </div>
            </div>
            <div class="remote-btn-row">
              <button
                type="button"
                class="vx-btn vx-btn-sm remote-cfg-btn"
                class:vx-primary={!remoteTestedSuccessfully}
                onclick={(e) => {
                  e.stopPropagation();
                  pickEngine("remote-openai");
                  showRemoteModal = true;
                }}
              >
                {#if remoteTestedSuccessfully}
                  ⚙ Configure Server
                {:else}
                  ⚡ Setup & Test
                {/if}
              </button>
            </div>

            <div class="metrics">
              {#each m.rows as row}
                <div class="metric" class:dim={row.dim}>
                  <span class="metric-label">{row.label}</span>
                  <div class="vx-meter">
                    <div style:width="{row.pct}%" style:background={row.color}></div>
                  </div>
                  <span class="metric-value">{row.value}</span>
                </div>
              {/each}
            </div>
          {:else}
            <div class="vx-label">model size</div>
            <div class="sizes">
              {#each engine.models as model}
                <button
                  class="size"
                  class:on={isSelected(engine.id, model)}
                  onclick={(e) => {
                    e.stopPropagation();
                    pickModel(engine.id, model);
                  }}
                >
                  <span class="size-id">{model.id}</span>
                  <span class="size-mb">
                    {formatSize(model.mb)}{downloadedFor(engine.id, model) ? " ✔" : ""}
                  </span>
                </button>
              {/each}
            </div>

            <div class="metrics">
              {#each m.rows as row}
                <div class="metric" class:dim={row.dim}>
                  <span class="metric-label">{row.label}</span>
                  <div class="vx-meter">
                    <div style:width="{row.pct}%" style:background={row.color}></div>
                  </div>
                  <span class="metric-value">{row.value}</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>

        <div class="right-col">
          <div class="spark">
            <div class="spark-head"><span>◌ quiet room</span><span>{m.quietPct}</span></div>
            <div class="spark-bars">
              {#each m.quiet as b}
                <div style:height="{b.h}%" style:opacity={b.o}></div>
              {/each}
            </div>
          </div>
          <div class="spark">
            <div class="spark-head"><span>≋ noisy room</span><span>{m.noisyPct}</span></div>
            <div class="spark-bars">
              {#each m.noisy as b}
                <div style:height="{b.h}%" style:opacity={b.o}></div>
              {/each}
            </div>
          </div>
        </div>
      </div>
    {/each}
  </div>

  <div class="status-row">
    {#if downloading}
      <span class="vx-pill vx-busy"><span class="vx-spinner"></span> Downloading {downloading} — this can take a few minutes</span>
    {:else if downloadError}
      <span class="vx-pill vx-err">✕ Download failed: {downloadError}</span>
    {:else if !readinessChecked}
      <span class="vx-pill vx-busy"><span class="vx-spinner"></span> Checking local models…</span>
    {:else if selectedEngine === "remote-openai"}
      {#if remoteTestedSuccessfully}
        <span class="vx-pill vx-ok">✓ Remote engine connected ({selectedModel})</span>
      {:else}
        <span class="vx-pill vx-warn">⚠️ Test connection required before continuing</span>
      {/if}
    {:else if selectedReady}
      <span class="vx-pill vx-ok">✓ {selectedModel} is on disk and ready</span>
    {:else if !wizard.engineChosen || !wizard.modelChosen}
      <span class="vx-pill">
        ◇ Pick an engine and a model size — they download before the next step
      </span>
    {:else}
      <span class="vx-pill">↓ {selectedModel} will download when you continue</span>
    {/if}
  </div>

  <RemoteEngineModal
    bind:isOpen={showRemoteModal}
    onSuccess={() => {
      remoteTestedSuccessfully = true;
    }}
  />
</div>

<style>
  .engine-step {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .head {
    display: flex;
    gap: 28px;
    align-items: flex-end;
    justify-content: space-between;
    flex: none;
    min-width: 0;
  }

  .copy {
    max-width: 760px;
    min-width: 0;
  }

  .gpu-toggle {
    flex: none;
    width: 400px;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 12px 16px;
  }

  /* A CPU-only build has nothing to switch on. Dimmed and not clickable, so it
     reads as a report about the build rather than a setting left turned off. */
  .gpu-toggle:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .switch {
    width: 50px;
    height: 30px;
    border-radius: 999px;
    position: relative;
    flex: none;
    background: var(--vx-bg-4);
    border: 1px solid var(--vx-line-2);
    transition: background 0.3s;
  }

  .switch.on {
    background: var(--vx-cyan-0);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 24px;
    height: 24px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 2px 6px rgba(0, 0, 0, 0.4);
    transition: left 0.28s var(--vx-ease);
  }

  .switch.on .knob {
    left: 23px;
  }

  .gpu-copy {
    display: block;
  }

  .gpu-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 600;
    font-size: 14px;
  }

  .gpu-title .glyph {
    font-family: var(--vx-mono);
    color: var(--vx-cyan-1);
  }

  .state {
    font-family: var(--vx-mono);
    font-size: 10.5px;
    letter-spacing: 0.1em;
    color: var(--vx-txt-3);
    transition: color 0.3s;
  }

  .state.on {
    color: var(--vx-cyan-1);
  }

  .gpu-desc {
    display: block;
    font-size: 12px;
    color: var(--vx-txt-2);
    margin-top: 2px;
    line-height: 1.4;
  }

  .engines {
    flex: 1;
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 14px;
  }

  .engine-card {
    position: relative;
    padding: 20px 22px;
    border-radius: 18px;
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 18px 22px;
    align-content: start;
  }

  .corner {
    position: absolute;
    top: 16px;
    right: 16px;
    width: 26px;
    height: 26px;
  }

  .engine-head {
    grid-column: 1 / -1;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 6px;
  }

  .engine-glyph {
    font-family: var(--vx-mono);
    font-size: 24px;
    color: var(--vx-cyan-1);
  }

  .engine-name {
    font-size: 21px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }

  .tagline {
    font-size: 13px;
    color: var(--vx-txt-2);
    line-height: 1.45;
    max-width: 90%;
  }

  .warn {
    margin-top: 10px;
    padding: 8px 11px;
    border-radius: 9px;
    border: 1px solid rgba(255, 180, 84, 0.3);
    background: rgba(255, 180, 84, 0.06);
    color: var(--vx-warn);
    font-size: 12px;
    line-height: 1.45;
  }

  .sizes {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
    margin-top: 6px;
  }

  .size {
    height: 46px;
    padding: 0 13px;
    border-radius: 10px;
    border: 1px solid var(--vx-line);
    background: rgba(255, 255, 255, 0.02);
    color: var(--vx-txt-1);
    font-family: var(--vx-mono);
    font-size: 12.5px;
    cursor: pointer;
    transition: all 0.22s;
    display: flex;
    flex-direction: column;
    justify-content: center;
    align-items: center;
    gap: 1px;
    line-height: 1.1;
  }

  .size:hover {
    border-color: var(--vx-line-2);
  }

  .size.on {
    border-color: var(--vx-cyan-b);
    background: rgba(34, 212, 239, 0.12);
    color: var(--vx-cyan-1);
  }

  .size-mb {
    font-size: 10px;
    opacity: 0.7;
  }

  .remote-summary-box {
    margin-top: 6px;
    padding: 10px 12px;
    background: rgba(0, 0, 0, 0.25);
    border: 1px solid var(--vx-line);
    border-radius: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .remote-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    font-family: var(--vx-mono);
    font-size: 11px;
  }

  .remote-k {
    color: var(--vx-txt-2);
    flex: none;
  }

  .remote-v {
    color: var(--vx-txt-0);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 190px;
    text-align: right;
  }

  .remote-v-ok {
    color: var(--vx-good);
    font-weight: 600;
  }

  .remote-v-warn {
    color: var(--vx-warn);
    font-weight: 600;
  }

  .remote-btn-row {
    margin-top: 8px;
  }

  .remote-cfg-btn {
    width: 100%;
    height: 36px;
    font-size: 12px;
  }

  .metrics {
    margin-top: 16px;
    display: flex;
    flex-direction: column;
    gap: 9px;
  }

  .metric {
    display: grid;
    grid-template-columns: 64px 1fr 58px;
    align-items: center;
    gap: 10px;
    transition: opacity 0.3s;
  }

  .metric.dim {
    opacity: 0.45;
  }

  .metric-label {
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-2);
  }

  .metric-value {
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-1);
    text-align: right;
  }

  .right-col {
    display: grid;
    grid-template-rows: 1fr 1fr;
    gap: 10px;
  }

  .spark {
    padding: 10px 12px;
    border-radius: 12px;
    border: 1px solid var(--vx-line);
    background: rgba(0, 0, 0, 0.25);
    display: flex;
    flex-direction: column;
  }

  .spark-head {
    display: flex;
    justify-content: space-between;
    font-family: var(--vx-mono);
    font-size: 11px;
    color: var(--vx-txt-2);
    margin-bottom: 6px;
  }

  .spark-head span:last-child {
    color: var(--vx-txt-0);
  }

  .spark-bars {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    flex: 1;
    min-height: 28px;
  }

  .spark-bars > div {
    flex: 1;
    border-radius: 2px;
    background: var(--vx-cyan-0);
    transition: height 0.6s var(--vx-ease), opacity 0.4s;
  }

  .status-row {
    flex: none;
    display: flex;
    justify-content: flex-end;
  }

  @media (max-width: 1100px) {
    .head {
      flex-direction: column;
      align-items: stretch;
      gap: 14px;
    }

    .gpu-toggle {
      width: 100%;
    }

    .engines {
      grid-template-columns: 1fr;
    }
  }
</style>
