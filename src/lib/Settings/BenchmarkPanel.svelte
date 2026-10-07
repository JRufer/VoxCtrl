<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount, onDestroy } from "svelte";

  /** Called with the engine → device choices the user accepts. */
  let { onApply } = $props<{
    onApply: (recommended: Record<string, "cpu" | "gpu">) => void;
  }>();

  interface Step {
    engine: string;
    device: "cpu" | "gpu";
    model: string;
    label: string;
    est_secs: number;
  }
  interface Result {
    engine: string;
    device: "cpu" | "gpu";
    model: string;
    load_ms: number | null;
    median_ms: number | null;
    error: string | null;
  }
  interface Progress {
    step: number;
    total_steps: number;
    label: string;
    phase: string;
    fraction: number;
  }

  const TITLES: Record<string, string> = {
    whisper: "Whisper.cpp",
    moonshine: "Moonshine",
    parakeet: "Parakeet",
    s1_mini: "S1-mini cleanup",
  };

  interface Skipped {
    engine: string;
    reason: string;
  }

  let steps = $state<Step[]>([]);
  let skipped = $state<Skipped[]>([]);
  let estSecs = $state(0);
  let planned = $state(false);

  let running = $state(false);
  let cancelling = $state(false);
  let progress = $state<Progress | null>(null);
  let elapsed = $state(0);
  let error = $state<string | null>(null);

  let results = $state<Result[]>([]);
  let recommended = $state<Record<string, "cpu" | "gpu">>({});
  let applied = $state(false);

  let timer: ReturnType<typeof setInterval> | null = null;
  let startedAt = 0;
  let unlisten: (() => void) | null = null;

  /** "about 45 seconds", "about 2 minutes". Rounded up so it errs long. */
  function describe(secs: number): string {
    if (secs < 50) return `about ${Math.max(10, Math.ceil(secs / 5) * 5)} seconds`;
    const mins = Math.ceil(secs / 30) / 2;
    return mins <= 1 ? "about a minute" : `about ${mins} minutes`;
  }

  /** Time left: the estimate until enough of the run is done to extrapolate
   *  from its actual pace, which tracks the machine better than the guess. */
  const remainingSecs = $derived.by(() => {
    const f = progress?.fraction ?? 0;
    if (f < 0.08) return Math.max(0, estSecs - elapsed);
    return Math.max(0, (elapsed * (1 - f)) / f);
  });

  const engines = $derived(
    [...new Set(results.map((r) => r.engine))].map((engine) => {
      const cpu = results.find((r) => r.engine === engine && r.device === "cpu");
      const gpu = results.find((r) => r.engine === engine && r.device === "gpu");
      return { engine, model: (cpu ?? gpu)?.model ?? "", cpu, gpu };
    }),
  );

  const fmtMs = (r?: Result) =>
    r?.median_ms != null ? `${Math.round(r.median_ms)} ms` : r?.error ? "failed" : "—";

  function verdict(e: { cpu?: Result; gpu?: Result; engine: string }): string {
    const c = e.cpu?.median_ms;
    const g = e.gpu?.median_ms;
    if (c == null || g == null) return "Could not compare";
    const choice = recommended[e.engine];
    if (choice === "gpu") return `GPU is ${(c / g).toFixed(1)}× faster`;
    if (g > c) return `CPU is ${(g / c).toFixed(1)}× faster`;
    return "About the same — CPU";
  }

  async function loadPlan() {
    try {
      const plan = await invoke<{ steps: Step[]; skipped: Skipped[]; est_secs: number }>("benchmark_plan");
      steps = Array.isArray(plan?.steps) ? plan.steps : [];
      skipped = Array.isArray(plan?.skipped) ? plan.skipped : [];
      estSecs = plan?.est_secs ?? 0;
    } catch (e) {
      console.error("benchmark_plan failed", e);
      steps = [];
    }
    planned = true;
  }

  async function start() {
    error = null;
    applied = false;
    results = [];
    recommended = {};
    cancelling = false;
    progress = { step: 0, total_steps: steps.length, label: steps[0]?.label ?? "", phase: "Starting", fraction: 0 };
    elapsed = 0;
    startedAt = Date.now();
    running = true;
    timer = setInterval(() => (elapsed = (Date.now() - startedAt) / 1000), 250);
    try {
      const out = await invoke<{
        results: Result[];
        recommended: [string, "cpu" | "gpu"][];
        cancelled: boolean;
      }>("run_benchmark");
      results = out.results;
      recommended = Object.fromEntries(out.recommended);
    } catch (e) {
      error = String(e);
    } finally {
      running = false;
      cancelling = false;
      if (timer) clearInterval(timer);
      timer = null;
    }
  }

  async function cancel() {
    cancelling = true;
    await invoke("cancel_benchmark");
  }

  function apply() {
    onApply(recommended);
    applied = true;
  }

  onMount(async () => {
    unlisten = await listen<Progress>("benchmark-progress", (e) => {
      progress = e.payload;
    });
    await loadPlan();
  });
  onDestroy(() => {
    unlisten?.();
    if (timer) clearInterval(timer);
  });
</script>

{#snippet skippedList()}
  {#if skipped.length > 0}
    <ul class="bench-skipped">
      {#each skipped as s}
        <li><strong>{TITLES[s.engine] ?? s.engine}</strong> is not tested: {s.reason}.</li>
      {/each}
    </ul>
  {/if}
{/snippet}

<div class="field-group">
  <h3>Speed test</h3>

  {#if !planned}
    <p class="hint">Checking what can be tested…</p>
  {:else if steps.length === 0}
    <p class="hint">
      Nothing to compare: the speed test needs an engine with a GPU path in this
      build and its model downloaded. Download a model above, then come back.
    </p>
    {@render skippedList()}
  {:else}
    <p class="hint">
      Whether the GPU helps depends on your machine — it can be several times
      faster for one engine and several times slower for another. This times
      each downloaded engine on the CPU and on the GPU with a short speech clip
      and recommends the faster one. It takes {describe(estSecs)} for
      {steps.length} tests; avoid dictating while it runs.
    </p>

    {#if running}
      <div class="bench-progress" role="progressbar" aria-valuemin="0" aria-valuemax="100"
        aria-valuenow={Math.round((progress?.fraction ?? 0) * 100)}>
        <div class="bench-bar"><div class="bench-fill" style="width: {(progress?.fraction ?? 0) * 100}%"></div></div>
        <div class="bench-meta">
          <span>
            {#if cancelling}Stopping after this test…{:else}
              Test {Math.min((progress?.step ?? 0) + 1, steps.length)} of {steps.length}
              — {progress?.label}: {progress?.phase}
            {/if}
          </span>
          <span>{Math.round((progress?.fraction ?? 0) * 100)}% · about {Math.max(1, Math.ceil(remainingSecs))} s left</span>
        </div>
      </div>
      <div class="bench-actions">
        <button class="btn-download" onclick={cancel} disabled={cancelling}>Cancel</button>
      </div>
    {:else}
      <div class="bench-actions">
        <button class="btn-download" onclick={start}>Test this machine (~{describe(estSecs).replace("about ", "")})</button>
      </div>
    {/if}

    {#if error}
      <p class="field-error-msg">{error}</p>
    {/if}

    {#if !running}
      {@render skippedList()}
    {/if}

    {#if !running && engines.length > 0}
      <table class="bench-table">
        <thead>
          <tr><th>Engine</th><th>CPU</th><th>GPU</th><th>Result</th></tr>
        </thead>
        <tbody>
          {#each engines as e}
            <tr>
              <td>{TITLES[e.engine] ?? e.engine} <span class="bench-model">{e.model}</span></td>
              <td class:win={recommended[e.engine] === "cpu"}>{fmtMs(e.cpu)}</td>
              <td class:win={recommended[e.engine] === "gpu"}>{fmtMs(e.gpu)}</td>
              <td>{verdict(e)}</td>
            </tr>
            {#if e.cpu?.error || e.gpu?.error}
              <tr><td colspan="4" class="bench-error">{e.cpu?.error ?? e.gpu?.error}</td></tr>
            {/if}
          {/each}
        </tbody>
      </table>
      <p class="hint">Time to process a short clip (lower is better). Model loading is not counted.</p>
      {#if Object.keys(recommended).length > 0}
        <div class="bench-actions">
          <button class="btn-download" onclick={apply} disabled={applied}>
            {applied ? "✔ Applied" : "Apply fastest settings"}
          </button>
        </div>
      {/if}
    {/if}
  {/if}
</div>

<style lang="postcss">
  @reference "../../app.css";

  .bench-actions {
    @apply flex items-center gap-3 mt-2;
  }
  .btn-download {
    @apply bg-[var(--accent)] border-none text-white rounded-[var(--radius)] p-1.5 px-3 text-xs cursor-pointer font-semibold transition-colors duration-200;
  }
  .btn-download:hover:not(:disabled) {
    @apply bg-[var(--accent2)];
  }
  .btn-download:disabled {
    @apply opacity-60 cursor-default;
  }
  .bench-progress {
    @apply mt-2 flex flex-col gap-1.5;
  }
  .bench-bar {
    @apply w-full h-2 rounded-full bg-[var(--bg)] border border-[var(--border)] overflow-hidden;
  }
  .bench-fill {
    @apply h-full bg-[var(--accent2)] transition-[width] duration-300 ease-out;
  }
  .bench-meta {
    @apply flex justify-between gap-3 text-xs text-[var(--text-muted)];
  }
  .bench-table {
    @apply w-full text-xs mt-3 border-collapse;
  }
  .bench-table th {
    @apply text-left font-medium text-[var(--text-muted)] pb-1.5 border-b border-[var(--border)];
  }
  .bench-table td {
    @apply py-1.5 pr-3 border-b border-[var(--border)];
  }
  .bench-table td.win {
    @apply text-emerald-300 font-semibold;
  }
  .bench-model {
    @apply text-[var(--text-muted)] ml-1;
  }
  .bench-skipped {
    @apply list-none m-0 mt-2 p-0 text-xs text-[var(--text-muted)] flex flex-col gap-0.5;
  }
  .bench-error {
    @apply text-red-300;
  }
</style>
