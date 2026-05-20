<script lang="ts">
  const version = "0.1.0";
</script>

<section>
  <h2>About VoxCtr</h2>

  <div class="field-group about-card">
    <div class="logo">🎙</div>
    <div>
      <div class="app-name">VoxCtr</div>
      <div class="app-version">Version {version} — Rust + Tauri Edition</div>
      <p>
        Native, on-device voice-to-text for Linux and Windows.
        Uses <a href="https://github.com/ggerganov/whisper.cpp" target="_blank">whisper.cpp</a>
        for offline transcription and routes speech to any destination.
      </p>
    </div>
  </div>

  <div class="field-group">
    <h3>System</h3>
    <div class="kv"><span>Frontend</span><span>Svelte 5 + Tauri 2</span></div>
    <div class="kv"><span>Backend</span><span>Rust (Tokio async)</span></div>
    <div class="kv"><span>Inference</span><span>whisper.cpp (subprocess)</span></div>
    <div class="kv"><span>Config</span><span><code>~/.config/voxctl/</code></span></div>
    <div class="kv"><span>Models</span><span><code>~/.local/share/voxctl/models/</code></span></div>
    <div class="kv"><span>MCP socket</span><span><code>/tmp/voxctl-mcp.sock</code></span></div>
  </div>

  <div class="field-group">
    <h3>Links</h3>
    <a href="https://github.com/jrufer/voxctr" target="_blank">GitHub Repository</a>
  </div>
</section>

<style>
  @import "./tab.css";
  .about-card {
    flex-direction: row !important;
    align-items: flex-start;
    gap: 20px;
  }
  .logo { font-size: 48px; }
  .app-name { font-size: 22px; font-weight: 700; margin-bottom: 4px; }
  .app-version { font-size: 12px; color: var(--text-muted); margin-bottom: 10px; }
  p { font-size: 13px; color: var(--text-muted); line-height: 1.6; max-width: 400px; }
  a { color: var(--accent2); font-size: 13px; text-decoration: none; }
  a:hover { text-decoration: underline; }
  .kv {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    padding: 3px 0;
    border-bottom: 1px solid var(--border);
  }
  .kv span:first-child { color: var(--text-muted); }
</style>
