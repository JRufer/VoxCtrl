<script lang="ts">
  import appIcon from "../../assets/app_icon.png";
  import { config, saveConfig, configDirty } from "../../stores/config";
  import { status } from "../../stores/status";
  import { invoke } from "@tauri-apps/api/core";

  import GeneralTab from "./GeneralTab.svelte";
  import EngineTab from "./EngineTab.svelte";
  import AudioTab from "./AudioTab.svelte";
  import RoutingTab from "./RoutingTab.svelte";
  import TtsTab from "./TtsTab.svelte";
  import OllamaTab from "./OllamaTab.svelte";
  import FeaturesTab from "./FeaturesTab.svelte";
  import AboutTab from "./AboutTab.svelte";

  type Tab = "general" | "engine" | "audio" | "features" | "routing" | "tts" | "ollama" | "about";
  let activeTab = $state<Tab>("general");

  const tabs: { id: Tab; label: string; icon: string }[] = [
    { id: "general",  label: "General",  icon: "⚙️" },
    { id: "engine",   label: "Engine",   icon: "🧠" },
    { id: "audio",    label: "Audio",    icon: "🔊" },
    { id: "features", label: "Features", icon: "✨" },
    { id: "routing",  label: "Routing",  icon: "🔀" },
    { id: "tts",      label: "TTS",      icon: "🗣️" },
    { id: "ollama",   label: "Ollama",   icon: "🦙" },
    { id: "about",    label: "About",    icon: "ℹ️" },
  ];

  async function handleSave() {
    await saveConfig($config);
  }

  async function toggleRecording() {
    await invoke("toggle_recording");
  }

  let lastTab = activeTab;
  $effect(() => {
    const currentTab = activeTab;
    if (currentTab !== lastTab) {
      lastTab = currentTab;
      if ($config?.audio?.dynamic_stream && $status.recording) {
        invoke("stop_recording").catch((e) => {
          console.error("Failed to stop recording on activeTab change:", e);
        });
      }
    }
  });
</script>

<div class="settings-root">
  <!-- Unified Premium Sidebar -->
  <aside class="sidebar">
    <div class="brand">
      <img src={appIcon} class="brand-logo" alt="VoxCtr Icon" />
      <div class="brand-text">
        <span class="brand-name">VoxCtr</span>
        <span class="brand-tag">DEKTOP PANEL</span>
      </div>
    </div>

    <!-- Tab navigation -->
    <nav class="sidebar-nav">
      {#each tabs as tab}
        <button
          class="nav-btn"
          class:active={activeTab === tab.id}
          onclick={() => (activeTab = tab.id)}
        >
          <span class="nav-icon">{tab.icon}</span>
          <span class="nav-label">{tab.label}</span>
          {#if activeTab === tab.id}
            <div class="nav-active-pill"></div>
          {/if}
        </button>
      {/each}
    </nav>

    <!-- Recording Control & Status Center -->
    <div class="sidebar-footer">
      <div class="status-panel" class:recording={$status.recording} class:speaking={$status.speaking}>
        <div class="status-header">
          <div class="status-indicator">
            <span class="status-dot"></span>
            <span class="status-label">
              {$status.recording ? "Recording" : $status.speaking ? "Speaking" : "Idle"}
            </span>
          </div>
          <span class="word-count">{$status.word_count} words</span>
        </div>

        <!-- Custom springy record button -->
        <button
          class="btn-record"
          class:active={$status.recording}
          onclick={toggleRecording}
        >
          {#if $status.recording}
            <span class="pulse-ring"></span>
            <span class="btn-text">🛑 Stop Session</span>
          {:else}
            <span class="btn-text">🎙️ Record Speech</span>
          {/if}
        </button>
      </div>
    </div>
  </aside>

  <!-- Main Content Area -->
  <main class="content-container">
    <div class="tab-content">
      {#if activeTab === "general"}
        <GeneralTab bind:cfg={$config} />
      {:else if activeTab === "engine"}
        <EngineTab bind:cfg={$config} />
      {:else if activeTab === "audio"}
        <AudioTab bind:cfg={$config} />
      {:else if activeTab === "features"}
        <FeaturesTab bind:cfg={$config} />
      {:else if activeTab === "routing"}
        <RoutingTab />
      {:else if activeTab === "tts"}
        <TtsTab bind:cfg={$config} />
      {:else if activeTab === "ollama"}
        <OllamaTab bind:cfg={$config} />
      {:else if activeTab === "about"}
        <AboutTab />
      {/if}
    </div>

    <!-- Floating Snappy Unsaved Changes Save Engine -->
    {#if $configDirty}
      <div class="floating-save-container">
        <span class="save-hint">Unsaved changes detected</span>
        <button class="btn-save" onclick={handleSave}>
          💾 Save Configuration
        </button>
      </div>
    {/if}
  </main>
</div>

<style>
  .settings-root {
    display: flex;
    height: 100vh;
    width: 100vw;
    background: var(--bg);
    color: var(--text);
    overflow: hidden;
  }

  /* Redesigned Sidebar Base */
  .sidebar {
    display: flex;
    flex-direction: column;
    width: 170px;
    background: var(--color-obsidian-900);
    border-right: 1px solid var(--border);
    flex-shrink: 0;
    padding: 20px 8px;
    z-index: 10;
    box-shadow: 4px 0 24px rgba(0, 0, 0, 0.25);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 10px 24px 10px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.03);
    margin-bottom: 16px;
  }

  .brand-logo {
    width: 24px;
    height: 24px;
    object-fit: contain;
    filter: drop-shadow(0 2px 8px rgba(255, 107, 53, 0.3));
    animation: floating 4s ease-in-out infinite;
  }

  @keyframes floating {
    0%, 100% { transform: translateY(0px) rotate(0deg); }
    50% { transform: translateY(-3px) rotate(3deg); }
  }

  .brand-text {
    display: flex;
    flex-direction: column;
  }

  .brand-name {
    font-size: 18px;
    font-weight: 850;
    color: #fff;
    letter-spacing: -0.5px;
    line-height: 1.1;
  }

  .brand-tag {
    font-size: 8px;
    font-weight: 700;
    color: var(--color-accent-blue);
    letter-spacing: 0.15em;
    margin-top: 1px;
  }

  .sidebar-nav {
    display: flex;
    flex-direction: column;
    gap: 4px;
    flex: 1;
    overflow-y: auto;
  }

  /* Nav Links with Snappy Elastic Springs */
  .nav-btn {
    position: relative;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 8px 10px;
    border-radius: var(--radius);
    color: var(--color-obsidian-300);
    font-size: 13px;
    font-weight: 600;
    text-align: left;
    transition: var(--transition-snappy-fast);
    white-space: nowrap;
  }

  .nav-btn:hover {
    color: #fff;
    background: rgba(255, 255, 255, 0.03);
    transform: translateX(2px);
  }

  .nav-btn.active {
    color: var(--color-accent-blue);
    background: rgba(56, 189, 248, 0.08);
    transform: scale(1.01) translateX(2px);
  }

  .nav-btn:active {
    transform: scale(0.97) translateX(0);
  }

  .nav-icon {
    font-size: 15px;
    transition: transform 0.2s var(--ease-spring-out);
  }

  .nav-btn:hover .nav-icon {
    transform: scale(1.18) rotate(5deg);
  }

  .nav-active-pill {
    position: absolute;
    right: 0;
    top: 15%;
    height: 70%;
    width: 3px;
    background: var(--color-accent-blue);
    border-radius: 99px 0 0 99px;
    box-shadow: -2px 0 8px var(--color-accent-blue);
  }

  /* Record / Status Box Container */
  .sidebar-footer {
    padding-top: 16px;
    border-top: 1px solid rgba(255, 255, 255, 0.03);
  }

  .status-panel {
    background: var(--color-obsidian-950);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    transition: var(--transition-snappy);
    box-shadow: inset 0 2px 6px rgba(0, 0, 0, 0.3);
  }

  .status-panel.recording {
    border-color: rgba(255, 107, 53, 0.25);
    box-shadow: 0 4px 20px rgba(255, 107, 53, 0.1), inset 0 2px 6px rgba(0, 0, 0, 0.3);
  }

  .status-header {
    display: flex;
    flex-direction: column;
    gap: 4px;
    align-items: flex-start;
  }

  .status-indicator {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--color-obsidian-300);
    transition: var(--transition-snappy-fast);
  }

  .status-panel.recording .status-dot {
    background: var(--color-accent-tangerine);
    box-shadow: 0 0 8px var(--color-accent-tangerine);
    animation: heartBeat 1.2s infinite;
  }

  .status-panel.speaking .status-dot {
    background: var(--color-accent-green);
    box-shadow: 0 0 8px var(--color-accent-green);
  }

  @keyframes heartBeat {
    0%, 100% { transform: scale(1); }
    50% { transform: scale(1.4); }
  }

  .status-label {
    font-size: 11px;
    font-weight: 700;
    color: var(--color-obsidian-300);
  }

  .status-panel.recording .status-label {
    color: var(--color-accent-tangerine);
  }

  .word-count {
    font-size: 10px;
    color: rgba(255, 255, 255, 0.3);
    font-weight: 600;
  }

  /* Custom Spring Record CTA Button */
  .btn-record {
    position: relative;
    width: 100%;
    padding: 8px 6px;
    border-radius: var(--radius);
    background: rgba(255, 107, 53, 0.08);
    color: var(--color-accent-tangerine);
    font-size: 12px;
    font-weight: 750;
    text-align: center;
    border: 1px solid rgba(255, 107, 53, 0.2);
    box-shadow: 0 2px 6px rgba(255, 107, 53, 0.05);
    transition: var(--transition-snappy-fast);
    overflow: hidden;
    white-space: nowrap;
  }

  .btn-record:hover {
    background: var(--color-accent-tangerine);
    color: #fff;
    border-color: var(--color-accent-tangerine);
    transform: translateY(-1px);
    box-shadow: 0 4px 12px rgba(255, 107, 53, 0.25);
  }

  .btn-record:active {
    transform: scale(0.97) translateY(0px);
  }

  .btn-record.active {
    background: var(--color-accent-tangerine);
    color: #fff;
    border-color: var(--color-accent-tangerine);
    box-shadow: 0 4px 16px rgba(255, 107, 53, 0.35);
  }

  .pulse-ring {
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
    border-radius: var(--radius);
    border: 2px solid var(--color-accent-tangerine);
    animation: ripple 1.6s infinite ease-out;
    opacity: 0;
    pointer-events: none;
  }

  @keyframes ripple {
    0% { transform: scale(1); opacity: 0.5; }
    100% { transform: scale(1.15, 1.3); opacity: 0; }
  }

  .btn-text {
    position: relative;
    z-index: 2;
  }

  /* Content area */
  .content-container {
    flex: 1;
    display: flex;
    flex-direction: column;
    background: var(--bg);
    overflow: hidden;
    position: relative;
    z-index: 1;
  }

  /* Elevate container above sidebar when any tab displays a modal */
  .content-container:has(:global(.modal-backdrop)) {
    z-index: 100;
  }

  .tab-content {
    flex: 1;
    overflow-y: auto;
    padding: 30px;
    z-index: 1;
  }

  /* Floating Snappy Save Change Banner */
  .floating-save-container {
    position: absolute;
    bottom: 24px;
    right: 24px;
    display: flex;
    align-items: center;
    gap: 14px;
    background: var(--color-obsidian-900);
    border: 1px solid var(--color-accent-blue);
    padding: 10px 16px;
    border-radius: var(--radius);
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4), 0 0 12px rgba(56, 189, 248, 0.15);
    z-index: 100;
    animation: popUp 0.3s cubic-bezier(0.175, 0.885, 0.32, 1.275) forwards;
  }

  @keyframes popUp {
    from { transform: translateY(20px) scale(0.9); opacity: 0; }
    to { transform: translateY(0) scale(1); opacity: 1; }
  }

  .save-hint {
    font-size: 12px;
    font-weight: 600;
    color: var(--color-obsidian-300);
  }

  .btn-save {
    background: var(--color-accent-blue);
    color: var(--color-obsidian-950);
    padding: 6px 14px;
    border-radius: 6px;
    font-size: 12px;
    font-weight: 750;
    transition: var(--transition-snappy-fast);
    box-shadow: 0 4px 10px rgba(56, 189, 248, 0.25);
  }

  .btn-save:hover {
    transform: scale(1.02) translateY(-1px);
    box-shadow: 0 6px 14px rgba(56, 189, 248, 0.35);
    filter: brightness(1.05);
  }

  .btn-save:active {
    transform: scale(0.97) translateY(0px);
  }
</style>
