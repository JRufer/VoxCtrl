<!--
  Per-app cleanup styling: a list of "app → styling" rules bound to
  `engine.s1_mini.app_styles`. The first rule whose app text appears in the
  focused app's name (case-insensitive) decides the styling for a dictation;
  otherwise the global styling applies (see S1MiniConfig::styling_for).

  "Detect app" counts down a few seconds so the user can click into the app
  they want a rule for, then adds a row with the name VoxCtrl sees for it,
  so nobody has to guess window classes.
-->
<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import CustomSelect from "./CustomSelect.svelte";
  import type { AppStyleRule } from "../../stores/config";

  let {
    rules = $bindable(),
    styleOptions,
    onchange,
  }: {
    rules: AppStyleRule[];
    styleOptions: { value: string; label: string }[];
    onchange?: () => void;
  } = $props();

  const DETECT_SECONDS = 3;
  let countdown = $state(0);
  let detectMessage = $state("");
  let detectFailed = $state(false);

  function changed() {
    onchange?.();
  }

  function addRow(app = "") {
    rules = [...rules, { app, styling: "casual" }];
    changed();
  }

  function removeRow(index: number) {
    rules = rules.filter((_, i) => i !== index);
    changed();
  }

  async function detect() {
    if (countdown > 0) return;
    detectMessage = "";
    detectFailed = false;
    countdown = DETECT_SECONDS;
    const timer = setInterval(() => {
      countdown = Math.max(0, countdown - 1);
    }, 1000);
    try {
      const app = await invoke<string | null>("detect_focused_app", { delayMs: DETECT_SECONDS * 1000 });
      if (app && app.trim()) {
        const name = app.trim();
        if (!rules.some((r) => r.app.trim().toLowerCase() === name.toLowerCase())) {
          addRow(name);
        }
        detectMessage = `Detected “${name}”.`;
      } else {
        detectFailed = true;
        detectMessage =
          "VoxCtrl couldn't tell which app was focused. On KDE, install kdotool; GNOME doesn't allow it. You can still type the app's name.";
      }
    } catch (e) {
      detectFailed = true;
      detectMessage = `Couldn't detect the app: ${e}`;
    } finally {
      clearInterval(timer);
      countdown = 0;
    }
  }
</script>

<div class="field-group app-styles">
  <div class="field-label-row">
    <div style="display: flex; flex-direction: column;">
      <h3 style="margin-bottom: 0;">Styling per app</h3>
      <p class="hint" style="margin-top: 4px;">
        Use a different styling when dictating into a particular app. The first matching rule wins; any other app
        uses the styling above.
      </p>
    </div>
    <div class="actions">
      <button class="btn-add-inline" type="button" onclick={detect} disabled={countdown > 0}>
        {#if countdown > 0}
          Click into the app… {countdown}
        {:else}
          ◎ Detect app
        {/if}
      </button>
      <button class="btn-add-inline" type="button" onclick={() => addRow()}>＋ Add app</button>
    </div>
  </div>

  {#if detectMessage}
    <p class="hint detect-message" class:failed={detectFailed} role="status">{detectMessage}</p>
  {/if}

  <div class="dynamic-list">
    {#each rules as rule, idx (idx)}
      <div class="dynamic-list-row">
        <input
          type="text"
          placeholder="App name, e.g. slack"
          aria-label="App name"
          bind:value={rules[idx].app}
          oninput={changed}
          style="flex: 1;"
        />
        <span style="color: var(--text-muted);">→</span>
        <div class="style-select">
          <CustomSelect bind:value={rules[idx].styling} options={styleOptions} onchange={changed} />
        </div>
        <button class="btn-remove-inline" type="button" aria-label="Remove rule" onclick={() => removeRow(idx)}
          >✕</button
        >
      </div>
    {/each}
    {#if rules.length === 0}
      <div class="empty-state" style="padding: 20px; grid-column: 1 / -1;">
        <p>No per-app styling yet. Every app uses the styling above.</p>
      </div>
    {/if}
  </div>
</div>

<style>
  .actions {
    display: flex;
    gap: 8px;
    flex-shrink: 0;
  }

  .style-select {
    flex: 0 0 190px;
    min-width: 0;
  }

  .detect-message {
    margin: 0 0 8px;
  }

  .detect-message.failed {
    color: var(--color-accent-tangerine);
  }
</style>
