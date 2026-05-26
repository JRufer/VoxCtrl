<script lang="ts">
  import appIcon from "../../assets/voxctr.gif";
  const version = "0.1.0";
</script>

<section>
  <h2>About VoxCtr</h2>

  <div class="field-group about-card">
    <img src={appIcon} class="logo animated-logo" alt="VoxCtr Logo" />
    <div>
      <div class="app-name">VoxCtr</div>
      <div class="app-version">Version {version} — Rust + Tauri Edition</div>
      <p>
        Native, on-device voice-to-text for Linux and Windows.
        Uses <a href="https://github.com/ggerganov/whisper.cpp" target="_blank">whisper.cpp</a>
        and <a href="https://github.com/usefulsensors/moonshine" target="_blank">Moonshine</a>
        for offline transcription and routes speech to any destination.
      </p>
    </div>
  </div>

  <div class="field-group">
    <h3>System</h3>
    <div class="kv"><span>Frontend</span><span>Svelte 5 + Tauri 2</span></div>
    <div class="kv"><span>Backend</span><span>Rust (Tokio async)</span></div>
    <div class="kv"><span>Inference</span><span>whisper.cpp & Moonshine</span></div>
    <div class="kv"><span>Config</span><span><code>~/.config/voxctl/</code></span></div>
    <div class="kv"><span>Models</span><span><code>~/.local/share/voxctl/models/</code></span></div>
    <div class="kv"><span>MCP socket</span><span><code>/tmp/voxctl-mcp.sock</code></span></div>
  </div>

  <div class="field-group">
    <h3>Open Source Attributions</h3>
    <p class="credits-hint">This application is built possible by these outstanding open-source projects:</p>
    <div class="credits-list">
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/ggerganov/whisper.cpp" target="_blank">whisper.cpp</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/usefulsensors/moonshine" target="_blank">Useful Sensors Moonshine</a>
        <span class="credit-license">Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://tauri.app" target="_blank">Tauri Framework</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://svelte.dev" target="_blank">Svelte & Vite</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/rhasspy/piper" target="_blank">Piper TTS</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://rust-lang.org" target="_blank">Rust, Tokio & CPAL</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
    </div>
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
  .logo {
    width: 64px;
    height: 64px;
    object-fit: contain;
    filter: drop-shadow(0 0 10px rgba(56, 189, 248, 0.6)) drop-shadow(0 4px 24px rgba(56, 189, 248, 0.35));
    border-radius: 12px;
  }
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

  /* Open Source Attribution styling */
  .credits-hint {
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 12px;
  }
  .credits-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 4px;
  }
  .credit-item {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
  .credit-item:last-child {
    border-bottom: none;
  }
  .credit-name-link {
    font-size: 13px;
    font-weight: 400;
    color: #fff;
    text-decoration: none;
    transition: color 0.15s ease;
  }
  .credit-name-link:hover {
    color: var(--color-accent-blue);
    text-decoration: underline;
  }
  .credit-license {
    font-size: 10px;
    background: rgba(56, 189, 248, 0.08);
    color: var(--color-accent-blue);
    padding: 2px 6px;
    border-radius: 4px;
    font-weight: 400;
    border: 1px solid rgba(56, 189, 248, 0.15);
  }
</style>
