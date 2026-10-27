<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import appIcon from "../../assets/app_icon.png";
  import { config, configDirty } from "../../stores/config";
  import type { OutputTarget } from "../Settings/routing-types";
  import CustomSelect from "../Settings/CustomSelect.svelte";

  type Entry = { id: number; at: number; target_id: string; target_label: string; text: string };
  type Status = {
    enabled: boolean;
    persisting: boolean;
    persist_error: string | null;
    key_store: string;
    count: number;
  };
  /** Where a Read aloud is: waiting for the voice to start, or playing. */
  type Speech = { id: number; phase: "preparing" | "speaking" };

  /** How long to wait for playback to start before giving up on it. A first
   * read loads the voice model, which can take a while on a slow machine. */
  const PREPARE_TIMEOUT_MS = 90_000;

  let entries = $state<Entry[]>([]);
  let status = $state<Status | null>(null);
  let query = $state("");
  let targets = $state<OutputTarget[]>([]);
  let confirmClear = $state(false);
  let notice = $state("");
  let sendTo = $state<Record<number, string>>({});
  let speech = $state<Speech | null>(null);
  let showPrivacy = $state(false);
  let prepareTimer: ReturnType<typeof setTimeout> | null = null;

  const AGE_OPTIONS = [
    { value: 1, label: "1 hour" },
    { value: 24, label: "24 hours" },
    { value: 168, label: "7 days" },
    { value: 720, label: "30 days" },
    { value: 0, label: "Until the limit pushes them out" },
  ];

  function markDirty() {
    config.set($config);
    configDirty.set(true);
  }

  async function refresh() {
    try {
      const view = await invoke<{ entries: Entry[]; status: Status }>("get_history", { query });
      entries = view.entries;
      status = view.status;
    } catch (e) {
      console.error("get_history failed:", e);
    }
  }

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function onSearch() {
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(refresh, 150);
  }

  function flash(message: string) {
    notice = message;
    setTimeout(() => {
      if (notice === message) notice = "";
    }, 4000);
  }

  async function copy(entry: Entry) {
    try {
      await navigator.clipboard.writeText(entry.text);
      flash("Copied to the clipboard.");
    } catch (e) {
      flash(`Couldn't copy: ${e}`);
    }
  }

  async function paste(entry: Entry) {
    try {
      await invoke("paste_history_entry", { id: entry.id });
    } catch (e) {
      flash(`Couldn't type it: ${e}`);
    }
  }

  async function send(entry: Entry, targetId: string) {
    if (!targetId) return;
    try {
      await invoke("send_history_entry", { id: entry.id, targetId });
      const label = targets.find((t) => t.id === targetId)?.label ?? targetId;
      flash(`Sent to ${label}.`);
    } catch (e) {
      flash(`Couldn't send it: ${e}`);
    }
    sendTo[entry.id] = "";
  }

  function speechDone() {
    if (prepareTimer) clearTimeout(prepareTimer);
    prepareTimer = null;
    speech = null;
  }

  async function speak(entry: Entry) {
    if (speech) return;
    speech = { id: entry.id, phase: "preparing" };
    prepareTimer = setTimeout(() => {
      if (speech?.phase === "preparing") {
        speechDone();
        flash("The voice didn't start. Check Settings → TTS.");
      }
    }, PREPARE_TIMEOUT_MS);
    try {
      await invoke("speak_history_entry", { id: entry.id });
    } catch (e) {
      speechDone();
      flash(`Couldn't read it aloud: ${e}`);
    }
  }

  async function stopSpeaking() {
    try {
      await invoke("stop_tts");
    } finally {
      speechDone();
    }
  }

  async function remove(entry: Entry) {
    await invoke("delete_history_entry", { id: entry.id });
    entries = entries.filter((e) => e.id !== entry.id);
  }

  async function clearAll() {
    await invoke("clear_history");
    confirmClear = false;
    await refresh();
  }

  function when(at: number): string {
    const d = new Date(at * 1000);
    const today = new Date();
    const sameDay = d.toDateString() === today.toDateString();
    const time = d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
    return sameDay ? time : `${d.toLocaleDateString([], { month: "short", day: "numeric" })}, ${time}`;
  }

  let sendOptions = $derived([
    { value: "", label: "Send to…" },
    ...targets.map((t) => ({ value: t.id, label: t.label || t.id })),
  ]);

  onMount(() => {
    refresh();
    invoke<OutputTarget[]>("get_targets")
      .then((t) => (targets = t))
      .catch(() => {});
    const unlisteners = [
      listen("history-changed", () => refresh()),
      listen("tts-playback-start", () => {
        if (speech) {
          if (prepareTimer) clearTimeout(prepareTimer);
          prepareTimer = null;
          speech = { ...speech, phase: "speaking" };
        }
      }),
      listen("tts-playback-end", () => speechDone()),
      listen<string>("tts-error", (e) => {
        if (speech) flash(`Couldn't read it aloud: ${e.payload}`);
        speechDone();
      }),
    ];
    return () => {
      if (prepareTimer) clearTimeout(prepareTimer);
      unlisteners.forEach((u) => u.then((f) => f()));
    };
  });
</script>

<main class="history-root">
  <header>
    <img src={appIcon} class="logo" alt="" />
    <div class="titles">
      <h1>History</h1>
      <p class="sub">
        {#if !$config.history?.enabled}
          History is off.
        {:else if status?.persisting}
          🔒 Kept between restarts, encrypted.
        {:else}
          Kept in memory only. Gone when VoxCtrl quits.
        {/if}
      </p>
    </div>
    <button
      class="btn-action small privacy-toggle"
      aria-expanded={showPrivacy}
      onclick={() => (showPrivacy = !showPrivacy)}>{showPrivacy ? "Done" : "Privacy settings"}</button
    >
  </header>

  {#if showPrivacy && $config.history}
    <section class="field-group privacy">
      <label class="field">
        <span>Keep a history of dictations</span>
        <input type="checkbox" bind:checked={$config.history.enabled} onchange={markDirty} />
      </label>

      {#if $config.history.enabled}
        <label class="field">
          <span>
            Keep history between restarts
            <small class="hint">
              Saved encrypted, with the key in {status?.key_store ?? "the system keyring"}, so other
              apps can't read the file. Turning this off deletes the file and the key.
            </small>
          </span>
          <input type="checkbox" bind:checked={$config.history.persist} onchange={markDirty} />
        </label>
        {#if status?.persist_error}
          <p class="warning-alert" role="alert">
            History isn't being saved: {status.persist_error}. It stays in memory only.
          </p>
        {/if}

        <label class="field">
          <span>Most dictations to keep</span>
          <input
            type="number"
            min="1"
            max="1000"
            bind:value={$config.history.max_entries}
            onchange={markDirty}
          />
        </label>

        <div class="field">
          <span>Forget dictations after</span>
          <CustomSelect
            bind:value={$config.history.max_age_hours}
            options={AGE_OPTIONS}
            onchange={markDirty}
          />
        </div>

        <label class="field">
          <span>
            Let MCP clients read recent dictations
            <small class="hint">
              Off by default. When on, any program that can connect to VoxCtrl's MCP server can read
              what you dictated.
            </small>
          </span>
          <input type="checkbox" bind:checked={$config.history.mcp_access} onchange={markDirty} />
        </label>
      {/if}
      <p class="hint">
        To keep a target's dictation out of history, open it in Settings → Output Commands and tick
        "Don't keep in history".
      </p>
    </section>
  {/if}

  {#if $config.history?.enabled !== false}
    <div class="toolbar">
      <input
        type="search"
        placeholder="Search history"
        aria-label="Search history"
        bind:value={query}
        oninput={onSearch}
      />
      {#if confirmClear}
        <button class="btn-action danger" onclick={clearAll}>Clear all {status?.count ?? ""}</button>
        <button class="btn-action" onclick={() => (confirmClear = false)}>Cancel</button>
      {:else}
        <button
          class="btn-action danger"
          onclick={() => (confirmClear = true)}
          disabled={!status?.count}>Clear all</button
        >
      {/if}
    </div>
    {#if notice}
      <p class="hint notice" role="status">{notice}</p>
    {/if}

    <div class="list-area">
      {#if entries.length === 0}
        <p class="hint empty">
          {query ? "Nothing in history matches that." : "Nothing dictated yet."}
        </p>
      {:else}
        <ul class="entries">
          {#each entries as entry (entry.id)}
            <li class="entry">
              <div class="meta">
                <span>{when(entry.at)}</span>
                <span class="target">→ {entry.target_label}</span>
              </div>
              <p class="text">{entry.text}</p>
              <div class="actions">
                <button class="btn-action small" onclick={() => copy(entry)}>Copy</button>
                <button
                  class="btn-action small"
                  title="Hide this window and type this into the window you were using"
                  onclick={() => paste(entry)}>Type again</button
                >
                {#if $config.tts?.enabled}
                  {#if speech?.id === entry.id && speech.phase === "preparing"}
                    <button class="btn-action small busy" disabled aria-busy="true">
                      <span class="spinner" aria-hidden="true"></span> Preparing voice…
                    </button>
                  {:else if speech?.id === entry.id}
                    <button class="btn-action small speaking" onclick={stopSpeaking}>
                      <span class="pulse" aria-hidden="true"></span> Speaking · Stop
                    </button>
                  {:else}
                    <button
                      class="btn-action small"
                      disabled={speech !== null}
                      title={speech ? "Another dictation is being read aloud" : undefined}
                      onclick={() => speak(entry)}>Read aloud</button
                    >
                  {/if}
                {/if}
                {#if targets.length > 0}
                  <div class="send">
                    <CustomSelect
                      bind:value={sendTo[entry.id]}
                      options={sendOptions}
                      defaultToFirst
                      onchange={(v) => send(entry, v)}
                    />
                  </div>
                {/if}
                <button
                  class="btn-action small danger"
                  aria-label="Delete this dictation"
                  onclick={() => remove(entry)}>Delete</button
                >
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  {:else}
    <p class="hint empty">
      History is off, so nothing you dictate is kept. Turn it on in Privacy settings above.
    </p>
  {/if}
</main>

<style lang="postcss">
  @reference "../../app.css";

  .history-root {
    @apply flex flex-col h-screen w-screen bg-[var(--bg)] text-[var(--text)] overflow-hidden p-5 gap-3;
  }
  header {
    @apply flex items-center gap-3;
  }
  .logo {
    @apply w-9 h-9 object-contain shrink-0;
  }
  .titles {
    @apply flex-1 min-w-0;
  }
  h1 {
    @apply text-lg font-extrabold text-white tracking-tight leading-tight m-0;
  }
  .sub {
    @apply text-[12px] text-[var(--text-muted)] m-0 mt-0.5;
  }
  .privacy {
    @apply shrink-0 max-h-[50vh] overflow-y-auto w-full max-w-none;
  }
  .privacy small.hint {
    @apply block font-normal mt-0.5;
  }
  .toolbar {
    @apply flex items-center gap-2 shrink-0;
  }
  .toolbar input[type="search"] {
    @apply flex-1 min-w-0;
  }
  .list-area {
    @apply flex-1 min-h-0 overflow-y-auto pr-1;
  }
  .entries {
    @apply flex flex-col gap-2 m-0 p-0 list-none;
  }
  .entry {
    @apply flex flex-col gap-1.5 border border-[var(--border)] rounded-[var(--radius)] p-3;
    background-color: rgba(255, 255, 255, 0.02);
  }
  .meta {
    @apply flex gap-2 text-[11px] text-[var(--text-muted)];
  }
  .meta .target {
    @apply truncate;
  }
  .text {
    @apply m-0 text-[13px] text-[var(--text)] whitespace-pre-wrap break-words;
  }
  .actions {
    @apply flex flex-wrap items-center gap-1.5;
  }
  /* Sized to sit in the row of small buttons rather than tower over it. */
  .send {
    @apply min-w-[140px];
  }
  .send :global(.custom-select-trigger) {
    @apply h-[26px] py-0 px-2 text-[11px] font-semibold;
  }
  .empty {
    @apply text-center py-6;
  }
  .btn-action.busy {
    @apply inline-flex items-center gap-1.5 opacity-100 cursor-progress;
  }
  .btn-action.speaking {
    @apply inline-flex items-center gap-1.5 border-[var(--color-accent-blue)] text-[var(--color-accent-blue)];
  }
  .spinner {
    @apply inline-block w-3 h-3 rounded-full border-2 border-current border-t-transparent;
    animation: spin 0.8s linear infinite;
  }
  .pulse {
    @apply inline-block w-2 h-2 rounded-full bg-current;
    animation: pulse 1s ease-in-out infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @keyframes pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.3;
    }
  }
</style>
