<script lang="ts">
  import type { AppConfig } from "../../stores/config";
  import { configDirty } from "../../stores/config";

  let { cfg = $bindable() }: { cfg: AppConfig } = $props();

  function markDirty() { configDirty.set(true); }
</script>

<section>
  <h2>General</h2>





  <div class="field-group">
    <h3>MCP Server</h3>
    <label class="field">
      <span>Enable MCP JSON-RPC server</span>
      <input type="checkbox" bind:checked={cfg.mcp.server_enabled} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Record timeout (seconds)</span>
      <input
        type="number"
        min="1"
        max="120"
        bind:value={cfg.mcp.record_timeout}
        onchange={markDirty}
      />
    </label>
    <p class="hint">Socket: <code>/tmp/voxctl-mcp.sock</code> (Linux) / <code>\\.\pipe\voxctl-mcp</code> (Windows)</p>
  </div>

  <div class="field-group">
    <h3>AT-SPI2 (Linux)</h3>
    <label class="field">
      <span>Use AT-SPI2 for text insertion</span>
      <input type="checkbox" bind:checked={cfg.atspi.injection} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Feed surrounding text as Whisper prompt</span>
      <input type="checkbox" bind:checked={cfg.atspi.context_prompt} onchange={markDirty} />
    </label>
    <label class="field">
      <span>Auto code mode in terminals / IDEs</span>
      <input type="checkbox" bind:checked={cfg.atspi.auto_code_mode} onchange={markDirty} />
    </label>
  </div>
</section>

<style>
  @import "./tab.css";
</style>
