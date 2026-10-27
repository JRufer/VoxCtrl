<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import type { AppConfig } from "../../stores/config";
  import { config, configDirty } from "../../stores/config";
  import type { OutputTarget } from "./routing-types";
  import CustomSelect from "./CustomSelect.svelte";

  let { cfg = $bindable() } = $props<{ cfg: AppConfig }>();

  type Entry = { id: number; at: number; target_id: string; target_label: string; text: string };
  type Status = {
    enabled: boolean;
    persisting: boolean;
    persist_error: string | null;
    key_store: string;
    count: number;
  };

  let entries = $state<Entry[]>([]);
  let status = $state<Status | null>(null);
  let query = $state("");
  let targets = $state<OutputTarget[]>([]);
  let confirmClear = $state(false);
  let notice = $state("");
  let sendTo = $state<Record<number, string>>({});

  const AGE_OPTIONS = [
    { value: 1, label: "1 hour" },
    { value: 24, label: "24 hours" },
    { value: 168, label: "7 days" },
    { value: 720, label: "30 days" },
    { value: 0, label: "Until the limit pushes them out" },
  ];

  function markDirty() {
    config.set(cfg);
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
    }, 2500);
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

  async function speak(entry: Entry) {
    try {
      await invoke("speak_text", { text: entry.text });
    } catch (e) {
      flash(`Couldn't read it aloud: ${e}`);
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
    const unlisten = listen("history-changed", () => refresh());
    return () => {
      unlisten.then((f) => f());
    };
  });
</script>

<section>
  <div>
    <h2>History</h2>
    <p class="hint">
      Your recent dictations, to copy, type again or send somewhere else. History is kept in
      memory and is gone when VoxCtrl quits, unless you choose to keep it below.
    </p>
  </div>

  <div class="field-group">
    <h3>Privacy</h3>
    <label class="field">
      <span>Keep a history of dictations</span>
      <input type="checkbox" bind:checked={cfg.history.enabled} onchange={markDirty} />
    </label>

    {#if cfg.history.enabled}
      <label class="field">
        <span>
          Keep history between restarts
          <small class="hint">
            Saved encrypted, with the key in {status?.key_store ?? "the system keyring"}, so other
            apps can't read the file. Turning this off deletes the file and the key.
          </small>
        </span>
        <input type="checkbox" bind:checked={cfg.history.persist} onchange={markDirty} />
      </label>
      {#if status?.persist_error}
        <p class="warning-alert" role="alert">
          History isn't being saved: {status.persist_error}. It stays in memory only.
        </p>
      {:else if status?.persisting}
        <p class="hint">🔒 Saved encrypted on this computer.</p>
      {/if}

      <label class="field">
        <span>Most dictations to keep</span>
        <input
          type="number"
          min="1"
          max="1000"
          bind:value={cfg.history.max_entries}
          onchange={markDirty}
        />
      </label>

      <div class="field">
        <span>Forget dictations after</span>
        <CustomSelect
          bind:value={cfg.history.max_age_hours}
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
        <input type="checkbox" bind:checked={cfg.history.mcp_access} onchange={markDirty} />
      </label>
    {/if}
    <p class="hint">
      To keep a target's dictation out of history, open it in Output Commands and tick
      "Don't keep in history".
    </p>
  </div>

  {#if cfg.history.enabled}
    <div class="field-group">
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
                  title="Hide Settings and type this into the window you were using"
                  onclick={() => paste(entry)}>Type again</button
                >
                {#if cfg.tts.enabled}
                  <button class="btn-action small" onclick={() => speak(entry)}>Read aloud</button>
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
  {/if}
</section>

<style lang="postcss">
  @reference "../../app.css";

  .toolbar {
    @apply flex items-center gap-2;
  }
  .toolbar input[type="search"] {
    @apply flex-1 min-w-0;
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
  .send {
    @apply min-w-[150px] text-[11px];
  }
  .empty {
    @apply text-center py-4;
  }
</style>
