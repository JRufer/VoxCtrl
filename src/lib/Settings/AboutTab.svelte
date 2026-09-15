<script lang="ts">
  import { onMount } from "svelte";
  import { getVersion } from "@tauri-apps/api/app";
  import { invoke } from "@tauri-apps/api/core";
  import appIcon from "../../assets/voxctrl.gif";

  let version = $state("0.1.0");
  let customOverlaysDir = $state("");
  let clonedTtsVoicesDir = $state("");

  onMount(async () => {
    try {
      version = await getVersion();
    } catch (e) {
      console.error("Failed to fetch app version:", e);
    }
    try {
      customOverlaysDir = await invoke<string>("get_custom_overlays_dir");
    } catch (e) {
      console.error("Failed to fetch custom overlays directory:", e);
    }
    try {
      clonedTtsVoicesDir = await invoke<string>("get_cloned_tts_voices_dir");
    } catch (e) {
      console.error("Failed to fetch cloned TTS voices directory:", e);
    }
  });
</script>

<section>
  <h2>About VoxCtrl</h2>

  <div class="field-group about-card">
    <img src={appIcon} class="logo animated-logo" alt="VoxCtrl Logo" />
    <div>
      <div class="app-name">VoxCtrl</div>
      <div class="app-version">Version {version} — Rust + Tauri Edition</div>
      <p>
        Native, on-device voice-to-text for Linux and Windows.
        Uses <a href="https://github.com/ggerganov/whisper.cpp" target="_blank">whisper.cpp</a>,
        <a href="https://github.com/usefulsensors/moonshine" target="_blank">Moonshine</a>,
        and <a href="https://github.com/NVIDIA/NeMo" target="_blank">NVIDIA Parakeet</a>
        for offline transcription and routes speech to any destination.
      </p>
    </div>
  </div>

  <div class="field-group">
    <h3>System</h3>
    <div class="kv"><span>Frontend</span><span>Svelte 5 + Tauri 2</span></div>
    <div class="kv"><span>Backend</span><span>Rust (Tokio async)</span></div>
    <div class="kv"><span>Inference</span><span>whisper.cpp, Moonshine & Parakeet TDT</span></div>
    <div class="kv"><span>Config</span><span><code>~/.config/voxctrl/</code></span></div>
    <div class="kv"><span>Models</span><span><code>~/.local/share/voxctrl/models/</code></span></div>
    {#if customOverlaysDir}
      <div class="kv"><span>Custom overlays</span><span><code>{customOverlaysDir}</code></span></div>
    {/if}
    {#if clonedTtsVoicesDir}
      <div class="kv"><span>Cloned TTS voices</span><span><code>{clonedTtsVoicesDir}</code></span></div>
    {/if}
    <div class="kv"><span>MCP socket</span><span><code>/tmp/voxctrl-mcp.sock</code></span></div>
  </div>

  <div class="field-group">
    <h3>Open Source Attributions</h3>
    <p class="credits-hint">This application is built possible by these outstanding open-source projects:</p>
    <p class="credits-hint">Speech recognition</p>
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
        <a class="credit-name-link" href="https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3" target="_blank">NVIDIA Parakeet TDT (model)</a>
        <span class="credit-license">CC-BY-4.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/microsoft/onnxruntime" target="_blank">ONNX Runtime</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
    </div>

    <p class="credits-hint">Text-to-speech</p>
    <div class="credits-list">
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/rhasspy/piper" target="_blank">Piper TTS</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/0xShug0/audio.cpp" target="_blank">audio.cpp</a>
        <span class="credit-license">Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/ggerganov/ggml" target="_blank">ggml</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://huggingface.co/kyutai/pocket-tts" target="_blank">Pocket-TTS (Kyutai Labs, model)</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://huggingface.co/openbmb/VoxCPM2" target="_blank">VoxCPM2 (openbmb)</a>
        <span class="credit-license">Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://huggingface.co/owensong/Inflect-Micro-v2" target="_blank">Inflect-Micro-v2</a>
        <span class="credit-license">Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/espeak-ng/espeak-ng" target="_blank">eSpeak NG (external, not bundled)</a>
        <span class="credit-license">GPL-3.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://huggingface.co/BreezeBlue/Breeze-TTS-2" target="_blank">Breeze-TTS-2 (BreezeBlue)</a>
        <span class="credit-license non-commercial">Non-Commercial License</span>
      </div>
    </div>

    <p class="credits-hint">Dictation cleanup</p>
    <div class="credits-list">
      <div class="credit-item">
        <a class="credit-name-link" href="https://huggingface.co/superwhisper/s1-mini" target="_blank">S1-mini (Superwhisper, Qwen3-0.6B base)</a>
        <span class="credit-license">Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://github.com/ggerganov/llama.cpp" target="_blank">llama.cpp</a>
        <span class="credit-license">MIT License</span>
      </div>
    </div>

    <p class="credits-hint">Core framework &amp; runtime</p>
    <div class="credits-list">
      <div class="credit-item">
        <a class="credit-name-link" href="https://tauri.app" target="_blank">Tauri Framework</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://svelte.dev" target="_blank">Svelte & Vite</a>
        <span class="credit-license">MIT License</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://codeberg.org/tazz4843/whisper-rs" target="_blank">whisper-rs</a>
        <span class="credit-license">Unlicense</span>
      </div>
      <div class="credit-item">
        <a class="credit-name-link" href="https://rust-lang.org" target="_blank">Rust, Tokio & CPAL</a>
        <span class="credit-license">MIT / Apache 2.0</span>
      </div>
    </div>
  </div>

  <div class="field-group">
    <h3>Links</h3>
    <a href="https://github.com/jrufer/voxctrl" target="_blank">GitHub Repository</a>
  </div>
</section>

<style>
  @reference "../../app.css";

  .about-card {
    @apply flex-row! items-start gap-5;
  }
  .logo {
    @apply w-16 h-16 object-contain drop-shadow-[0_0_10px_rgba(56,189,248,0.6)] drop-shadow-[0_4px_24px_rgba(56,189,248,0.35)] rounded-xl;
  }
  .app-name {
    @apply text-2xl font-bold mb-1;
  }
  .app-version {
    @apply text-xs text-[var(--text-muted)] mb-2.5;
  }
  p {
    @apply text-[13px] text-[var(--text-muted)] leading-relaxed max-w-[400px];
  }
  a {
    @apply text-[var(--accent2)] text-[13px] no-underline;
  }
  a:hover {
    @apply underline;
  }
  .kv {
    @apply flex justify-between text-xs py-1 border-b border-[var(--border)];
  }
  .kv span:first-child {
    @apply text-[var(--text-muted)];
  }

  /* Open Source Attribution styling */
  .credits-hint {
    @apply text-xs text-[var(--text-muted)] mb-3;
  }
  .credits-list {
    @apply flex flex-col gap-2 mt-1;
  }
  .credit-item {
    @apply flex justify-between items-center py-1.5 border-b border-[var(--border)] last:border-none;
  }
  .credit-name-link {
    @apply text-[13px] font-normal text-white no-underline transition-colors duration-150 ease-out;
  }
  .credit-name-link:hover {
    @apply text-[var(--color-accent-blue)] underline;
  }
  .credit-license {
    @apply text-[10px] bg-[var(--color-accent-blue)]/8 text-[var(--color-accent-blue)] p-0.5 px-1.5 rounded border border-[var(--color-accent-blue)]/15 font-normal;
  }
  /* Amber flag for anything that isn't a plain permissive OSS license:
     non-commercial terms, a gated model card, or a proprietary EULA. */
  .credit-license.non-commercial,
  .credit-license.flagged {
    @apply bg-amber-500/10 text-amber-400 border-amber-500/30;
  }
</style>
