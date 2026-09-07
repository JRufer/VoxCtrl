<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { open as openExternal } from "@tauri-apps/plugin-shell";
  import { save as saveDialog } from "@tauri-apps/plugin-dialog";

  type Limits = {
    cooldown_seconds: number;
    per_day: number;
    per_month: number;
    min_description_chars: number;
    max_description_chars: number;
  };

  type Context = {
    relay_configured: boolean;
    issues_new_url: string;
    support_email: string;
    log_path: string;
    install_id: string;
    limits: Limits;
    submissions_last_day: number;
    submissions_last_month: number;
  };

  type Preview = {
    markdown: string;
    title: string;
    fingerprint: string;
    blocked_reason: string | null;
    can_submit: boolean;
    github_url: string;
    mailto_url: string;
  };

  type Outcome = { ok: boolean; issue_url: string | null; message: string };

  type Statement = {
    summary: string;
    description: string;
    area: string;
    frequency: string;
  };

  const AREAS = [
    "Recording / hotkeys",
    "Transcription accuracy or speed",
    "Audio input or devices",
    "Text-to-speech",
    "Output targets / routing",
    "Settings window or overlay",
    "Installing or updating",
    "Crash or freeze",
    "Something else",
  ];

  const FREQUENCIES = [
    { value: "always", label: "Every time" },
    { value: "sometimes", label: "Sometimes" },
    { value: "once", label: "It happened once" },
  ];

  let statement = $state<Statement>({
    summary: "",
    description: "",
    area: AREAS[0],
    frequency: "always",
  });

  let context = $state<Context | null>(null);
  let preview = $state<Preview | null>(null);
  let previewing = $state(false);
  let previewError = $state<string | null>(null);
  let showPreview = $state(false);
  let sending = $state(false);
  let outcome = $state<Outcome | null>(null);
  let savedPath = $state<string | null>(null);
  let copied = $state(false);
  let identityReset = $state(false);

  const descriptionLength = $derived(statement.description.trim().length);
  const minChars = $derived(context?.limits.min_description_chars ?? 30);
  const maxChars = $derived(context?.limits.max_description_chars ?? 4000);
  // The summary is the issue title, so an empty one produces an untitled issue.
  const formComplete = $derived(
    statement.summary.trim().length > 0 &&
      descriptionLength >= minChars &&
      descriptionLength <= maxChars,
  );

  async function refreshContext() {
    try {
      context = await invoke<Context>("bug_report_context");
    } catch (e) {
      console.error("Failed to read bug report context:", e);
    }
  }

  onMount(refreshContext);

  // Rebuilding the report costs a config read and a log read, so it waits for a
  // pause in typing rather than running on every keystroke.
  let debounce: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const snapshot: Statement = { ...statement };
    if (!formComplete) {
      preview = null;
      return;
    }
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(() => buildPreview(snapshot), 400);
    return () => {
      if (debounce) clearTimeout(debounce);
    };
  });

  async function buildPreview(snapshot: Statement) {
    previewing = true;
    previewError = null;
    try {
      preview = await invoke<Preview>("preview_bug_report", { statement: snapshot });
    } catch (e) {
      previewError = String(e);
      preview = null;
    } finally {
      previewing = false;
    }
  }

  async function send() {
    if (!formComplete) return;
    sending = true;
    outcome = null;
    try {
      outcome = await invoke<Outcome>("submit_bug_report", {
        statement: { ...statement },
      });
    } catch (e) {
      outcome = { ok: false, issue_url: null, message: String(e) };
    } finally {
      sending = false;
      await refreshContext();
      if (formComplete) await buildPreview({ ...statement });
    }
  }

  async function saveToFile() {
    if (!formComplete) return;
    try {
      const suggested = await invoke<string>("suggested_bug_report_filename");
      const path = await saveDialog({
        defaultPath: suggested,
        filters: [{ name: "Markdown", extensions: ["md"] }],
      });
      if (!path) return;
      savedPath = await invoke<string>("save_bug_report", {
        statement: { ...statement },
        path,
      });
    } catch (e) {
      previewError = `Could not save the report: ${e}`;
    }
  }

  async function copyReport() {
    if (!preview) return;
    try {
      await navigator.clipboard.writeText(preview.markdown);
      copied = true;
      setTimeout(() => (copied = false), 2500);
    } catch (e) {
      previewError = `Could not copy the report: ${e}`;
    }
  }

  async function openLink(url: string) {
    try {
      await openExternal(url);
    } catch (e) {
      previewError = `Could not open that link: ${e}`;
    }
  }

  async function resetIdentity() {
    try {
      const fresh = await invoke<string>("reset_bug_report_identity");
      identityReset = true;
      setTimeout(() => (identityReset = false), 3000);
      if (context) context = { ...context, install_id: fresh, submissions_last_day: 0, submissions_last_month: 0 };
    } catch (e) {
      previewError = `Could not reset the report ID: ${e}`;
    }
  }
</script>

<section>
  <h2>Report a Bug</h2>

  <p class="lede">
    VoxCtrl 0.5.0 added a Windows build, and a Windows GPU build is on its way. Neither has been
    through anyone's hands but the developer's yet, so if something is broken, this page is the
    fastest way to say so — and it is the only part of VoxCtrl that ever sends anything about
    your machine anywhere.
  </p>

  <!-- The disclosure comes before the form on purpose: nobody should have to
       type a report to find out what pressing Send would do. -->
  <div class="field-group disclosure">
    <h3>What gets sent</h3>
    <div class="ledger">
      <div class="ledger-col included">
        <div class="ledger-head">✓ Included</div>
        <ul>
          <li><strong>What you write below.</strong> This is the only text of yours that travels.</li>
          <li><strong>Version and build</strong> — which VoxCtrl, installed how, with which GPU features compiled in.</li>
          <li><strong>Operating system</strong>, kernel or Windows build, and CPU architecture.</li>
          <li><strong>Hardware</strong> — CPU model, core count, total memory, display adapter name.</li>
          <li><strong>Desktop and session</strong> (Linux), and your language, e.g. <code>en</code>.</li>
          <li><strong>Settings</strong>, with everything below stripped out first.</li>
          <li><strong>Output targets and hotkeys</strong> — how many, of which kinds, which keys. Not their names or contents.</li>
          <li><strong>The tail of VoxCtrl's log</strong>, up to 400 lines.</li>
        </ul>
      </div>
      <div class="ledger-col excluded">
        <div class="ledger-head">✕ Never included</div>
        <ul>
          <li><strong>Anything you have dictated.</strong> The log is written so transcribed text cannot reach it in the first place.</li>
          <li><strong>API keys and access tokens.</strong> Reported only as set or not set.</li>
          <li><strong>Your name, username, hostname, email or IP address.</strong></li>
          <li><strong>File paths.</strong> A folder setting is reported as "default" or "custom", never as a path.</li>
          <li><strong>Custom vocabulary, snippets and prompts.</strong> Counted, never quoted.</li>
          <li><strong>Target names, shell commands, URLs and webhook secrets.</strong></li>
          <li><strong>Audio.</strong> No recording, no sample, ever.</li>
        </ul>
      </div>
    </div>
    <p class="hint">
      Nothing here happens on its own. There is no telemetry, no background reporting, and nothing
      is sent until you press a button on this page — and you can read the whole report first.
      The rules are enforced in code and tested in CI; the log this quotes is at
      <code>{context?.log_path ?? "…"}</code>.
    </p>
  </div>

  <div class="field-group">
    <h3>What went wrong</h3>

    <div class="field col">
      <label class="field-caption" for="bug-summary">One-line summary</label>
      <input
        id="bug-summary"
        type="text"
        class="text-input"
        maxlength="140"
        placeholder="e.g. Recording stops after about a second on Windows"
        bind:value={statement.summary}
      />
    </div>

    <div class="two-up">
      <div class="field col">
        <label class="field-caption" for="bug-area">Which part of VoxCtrl</label>
        <select id="bug-area" bind:value={statement.area}>
          {#each AREAS as area}
            <option value={area}>{area}</option>
          {/each}
        </select>
      </div>
      <div class="field col">
        <label class="field-caption" for="bug-frequency">How often</label>
        <select id="bug-frequency" bind:value={statement.frequency}>
          {#each FREQUENCIES as frequency}
            <option value={frequency.value}>{frequency.label}</option>
          {/each}
        </select>
      </div>
    </div>

    <div class="field col">
      <label class="field-caption" for="bug-description">
        What you did, what happened, and what you expected
      </label>
      <textarea
        id="bug-description"
        class="text-input description"
        maxlength={maxChars}
        placeholder={"1. I pressed my dictation hotkey and spoke for ten seconds.\n2. The overlay appeared but no text arrived.\n3. I expected the text to be typed into my editor."}
        bind:value={statement.description}
      ></textarea>
      <div class="counter" class:short={descriptionLength > 0 && descriptionLength < minChars}>
        {descriptionLength} / {maxChars} characters
        {#if descriptionLength < minChars}
          — at least {minChars} please; steps to reproduce are worth more than anything else here
        {/if}
      </div>
      <p class="hint">
        Write it as if to someone who cannot see your screen. Please do not paste logs or settings
        here — they are attached already.
      </p>
    </div>
  </div>

  {#if previewError}
    <div class="warning-alert">{previewError}</div>
  {/if}

  {#if preview?.blocked_reason}
    <div class="warning-alert">{preview.blocked_reason}</div>
  {/if}

  <div class="field-group">
    <div class="field-label-row">
      <h3 style="margin-bottom: 0;">The report</h3>
      {#if preview}
        <button class="btn-add-inline" type="button" onclick={() => (showPreview = !showPreview)}>
          {showPreview ? "Hide" : "Show me exactly what will be sent"}
        </button>
      {/if}
    </div>

    {#if !formComplete}
      <p class="hint">Fill in the summary and description above and the finished report appears here.</p>
    {:else if previewing && !preview}
      <p class="hint">Building the report…</p>
    {:else if preview}
      <p class="hint">
        Title: <strong>{preview.title}</strong> · Report ID
        <code>{preview.fingerprint.slice(0, 12)}</code>
      </p>
      {#if showPreview}
        <pre class="preview">{preview.markdown}</pre>
      {/if}

      <div class="actions">
        {#if context?.relay_configured}
          <button
            class="btn-primary"
            type="button"
            disabled={sending || !preview.can_submit}
            onclick={send}
          >
            {sending ? "Sending…" : "Send report"}
          </button>
        {/if}
        <button class="btn-secondary" type="button" onclick={() => openLink(preview!.github_url)}>
          Open on GitHub
        </button>
        <button class="btn-secondary" type="button" onclick={saveToFile}>
          Save report to a file
        </button>
        <button class="btn-secondary" type="button" onclick={copyReport}>
          {copied ? "Copied ✓" : "Copy report"}
        </button>
        <button class="btn-secondary" type="button" onclick={() => openLink(preview!.mailto_url)}>
          Email it
        </button>
      </div>

      <p class="hint">
        {#if context?.relay_configured}
          <strong>Send report</strong> files it for you — no GitHub account needed.
        {:else}
          This build has no automatic submission set up, so the routes below are the ones to use.
        {/if}
        <strong>Open on GitHub</strong> fills in the issue form in your browser and waits for you to
        press Submit there; it needs a free GitHub account, and nothing is sent until you do.
        <strong>Save</strong>, <strong>Copy</strong> and <strong>Email</strong> put the report in your
        hands to send however you like — attach the saved file to an email to
        <code>{context?.support_email ?? "the address on the project page"}</code>.
      </p>

      {#if savedPath}
        <div class="ok-alert">Saved to <code>{savedPath}</code></div>
      {/if}
    {/if}

    {#if outcome}
      <div class:ok-alert={outcome.ok} class:warning-alert={!outcome.ok}>
        {outcome.message}
        {#if outcome.issue_url}
          <button class="link-button" type="button" onclick={() => openLink(outcome!.issue_url!)}>
            View the issue
          </button>
        {/if}
      </div>
    {/if}
  </div>

  <div class="field-group">
    <h3>Limits and the ID that enforces them</h3>
    <p class="hint">
      To keep the issue tracker usable, sending is limited to
      {context?.limits.per_day ?? 5} reports a day and {context?.limits.per_month ?? 20} a month, with
      {Math.round((context?.limits.cooldown_seconds ?? 120) / 60)} minutes between them, and the same
      report twice is recognised and refused. You have sent
      {context?.submissions_last_day ?? 0} in the last day and
      {context?.submissions_last_month ?? 0} in the last month. Saving to a file is never limited.
    </p>
    <div class="field">
      <span>
        Report ID <code>{context?.install_id?.slice(0, 12) ?? "…"}</code> — a random value made on
        this machine, used only to count reports. It is derived from nothing about you or your
        computer, and resetting it starts a fresh count.
      </span>
      <button class="btn-secondary" type="button" onclick={resetIdentity}>
        {identityReset ? "Reset ✓" : "Reset ID"}
      </button>
    </div>
  </div>
</section>

<style>
  @reference "../../app.css";

  .lede {
    @apply text-[13px] leading-relaxed text-[var(--text-muted)];
  }

  .disclosure {
    @apply border-[var(--color-accent-blue)]/25;
    background-color: rgba(56, 189, 248, 0.04);
  }

  .ledger {
    @apply grid gap-4;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
  }

  .ledger-col ul {
    @apply flex flex-col gap-1.5 mt-2 text-[11.5px] leading-relaxed text-[var(--text-muted)];
  }

  .ledger-col li {
    @apply pl-3 border-l;
  }

  .ledger-head {
    @apply text-[11px] font-bold uppercase tracking-wider;
  }

  .included .ledger-head {
    @apply text-[var(--color-accent-green)];
  }

  .included li {
    border-color: rgba(16, 185, 129, 0.3);
  }

  .excluded .ledger-head {
    color: #f87171;
  }

  .excluded li {
    border-color: rgba(248, 113, 113, 0.3);
  }

  .ledger-col :global(strong) {
    @apply text-[var(--text)] font-semibold;
  }

  .field-caption {
    @apply text-[13px] font-medium text-[var(--text)];
  }

  .two-up {
    @apply grid gap-4;
    grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
  }

  .text-input {
    @apply w-full bg-[var(--bg)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-2 px-3 text-[13px] outline-none box-border transition-all duration-200 ease-out;
  }

  .text-input:focus {
    @apply border-[var(--accent2)] shadow-[0_0_0_2px_rgba(79,195,247,0.2)];
  }

  .description {
    @apply min-h-[150px] resize-y leading-relaxed;
  }

  .counter {
    @apply text-[11px] text-[var(--text-muted)] mt-1;
  }

  .counter.short {
    color: #f59e0b;
  }

  .preview {
    @apply w-full max-h-[340px] overflow-auto bg-[var(--color-obsidian-950)] border border-[var(--border)] rounded-[var(--radius)] p-3 text-[11px] leading-relaxed whitespace-pre-wrap break-words font-mono text-[var(--text-muted)];
  }

  .actions {
    @apply flex flex-wrap gap-2 mt-1;
  }

  .btn-primary {
    @apply bg-[var(--color-accent-blue)] text-white px-3.5 py-2 rounded-md text-xs font-bold transition-all duration-150 ease-out shadow-[0_4px_10px_rgba(56,189,248,0.25)];
  }

  .btn-primary:hover:not(:disabled) {
    @apply -translate-y-[1px] shadow-[0_6px_14px_rgba(56,189,248,0.35)] brightness-[1.05];
  }

  .btn-primary:disabled {
    @apply opacity-40 cursor-not-allowed shadow-none;
  }

  .btn-secondary {
    @apply px-3.5 py-2 rounded-md text-xs font-bold border border-[var(--border)] text-[var(--text)] bg-white/[0.03] transition-all duration-150 ease-out shrink-0;
  }

  .btn-secondary:hover {
    @apply bg-white/[0.08] border-[var(--color-accent-blue)]/40;
  }

  .link-button {
    @apply underline font-semibold ml-1;
  }

  .ok-alert {
    @apply flex items-center gap-2 text-xs font-medium mt-1;
    background-color: rgba(16, 185, 129, 0.1);
    border: 1px solid rgba(16, 185, 129, 0.25);
    color: #10b981;
    padding: 10px 14px;
    border-radius: var(--radius);
  }

  code {
    background-color: var(--color-obsidian-950);
    color: var(--color-accent-blue);
    padding: 2px 6px;
    border-radius: 4px;
    font-family: monospace;
    font-size: 10px;
    border: 1px solid var(--border);
  }
</style>
