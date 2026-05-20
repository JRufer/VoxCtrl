<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OutputTarget, HotkeyBinding } from "./routing-types";

  let targets = $state<OutputTarget[]>([]);
  let bindings = $state<HotkeyBinding[]>([]);
  let saving = $state(false);

  onMount(async () => {
    targets = await invoke<OutputTarget[]>("get_targets");
    bindings = await invoke<HotkeyBinding[]>("get_bindings");
  });

  async function saveAll() {
    saving = true;
    try {
      await invoke("save_targets", { targets });
      await invoke("save_bindings", { bindings });
    } finally {
      saving = false;
    }
  }

  function deliveryLabel(d: string) {
    const map: Record<string, string> = {
      inject: "Inject", clipboard: "Clipboard", exec: "Exec",
      pipe: "Pipe", socket: "Socket", file: "File",
      dbus: "DBus", http: "HTTP", webhook: "Webhook",
    };
    return map[d] ?? d;
  }

  function gestureLabel(g: string) {
    const map: Record<string, string> = {
      hold: "Hold", toggle: "Toggle", double_tap: "Double-Tap", chord: "Chord",
    };
    return map[g] ?? g;
  }
</script>

<section>
  <h2>Routing</h2>

  <div class="field-group">
    <div class="table-header">
      <h3>Output Targets</h3>
    </div>
    <table class="rt-table">
      <thead>
        <tr><th>ID</th><th>Label</th><th>Delivery</th><th>Newline</th></tr>
      </thead>
      <tbody>
        {#each targets as t}
          <tr>
            <td><code>{t.id}</code></td>
            <td>{t.label}</td>
            <td><span class="badge">{deliveryLabel(t.delivery)}</span></td>
            <td>{t.append_newline ? "✓" : ""}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    <p class="hint">Edit <code>~/.config/voxctl/targets.toml</code> directly for advanced configuration.</p>
  </div>

  <div class="field-group">
    <h3>Hotkey Bindings</h3>
    <table class="rt-table">
      <thead>
        <tr><th>ID</th><th>Keys</th><th>Gesture</th><th>Target</th><th>Enabled</th></tr>
      </thead>
      <tbody>
        {#each bindings as b}
          <tr>
            <td><code>{b.id}</code></td>
            <td><code>{b.keys.join(" + ")}</code></td>
            <td>{gestureLabel(b.gesture)}</td>
            <td>{b.target_id}</td>
            <td>{b.disabled ? "✗" : "✓"}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    <p class="hint">Edit <code>~/.config/voxctl/bindings.toml</code> for advanced configuration.</p>
  </div>

  <div class="actions">
    <button class="btn-save" onclick={saveAll} disabled={saving}>
      {saving ? "Saving…" : "Save Routing Config"}
    </button>
  </div>
</section>

<style>
  @import "./tab.css";
  .rt-table {
    width: 100%;
    border-collapse: collapse;
    font-size: 12px;
  }
  .rt-table th {
    text-align: left;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-weight: 600;
  }
  .rt-table td {
    padding: 6px 8px;
    border-bottom: 1px solid var(--border);
  }
  .badge {
    background: var(--surface2);
    border-radius: 4px;
    padding: 1px 6px;
    font-size: 11px;
  }
  .actions { display: flex; justify-content: flex-end; }
  .btn-save {
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    padding: 7px 18px;
    font-weight: 600;
    font-size: 13px;
  }
  .btn-save:disabled { opacity: 0.4; }
</style>
