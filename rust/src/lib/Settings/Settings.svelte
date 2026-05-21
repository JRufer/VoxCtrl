<script lang="ts">
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

  const tabs: { id: Tab; label: string }[] = [
    { id: "general",  label: "General" },
    { id: "engine",   label: "Engine" },
    { id: "audio",    label: "Audio" },
    { id: "features", label: "Features" },
    { id: "routing",  label: "Routing" },
    { id: "tts",      label: "TTS" },
    { id: "ollama",   label: "Ollama" },
    { id: "about",    label: "About" },
  ];

  async function handleSave() {
    await saveConfig($config);
  }

  async function toggleRecording() {
    await invoke("toggle_recording");
  }
</script>

<div class="settings-root">
  <!-- Header -->
  <header class="settings-header">
    <div class="brand">
      <span class="brand-icon">🎙</span>
      <span class="brand-name">VoxCtr</span>
    </div>
    <div class="status-bar">
      <span
        class="status-dot"
        class:recording={$status.recording}
        class:speaking={$status.speaking}
      ></span>
      <span class="status-label">
        {$status.recording ? "Recording" : $status.speaking ? "Speaking" : "Idle"}
      </span>
      <span class="word-count">{$status.word_count} words</span>
    </div>
    <button
      class="btn-record"
      class:active={$status.recording}
      onclick={toggleRecording}
    >
      {$status.recording ? "Stop" : "Record"}
    </button>
  </header>

  <div class="settings-body">
    <!-- Tab nav -->
    <nav class="tab-nav">
      {#each tabs as tab}
        <button
          class="tab-btn"
          class:active={activeTab === tab.id}
          onclick={() => (activeTab = tab.id)}
        >
          {tab.label}
        </button>
      {/each}
    </nav>

    <!-- Tab content -->
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
  </div>
</div>

<style>
  .settings-root {
    display: flex;
    flex-direction: column;
    height: 100vh;
    background: var(--bg);
    color: var(--text);
  }

  .settings-header {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 12px 20px;
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 16px;
    font-weight: 700;
  }

  .status-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
  }

  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-muted);
    transition: background 0.2s;
  }
  .status-dot.recording { background: var(--accent); animation: blink 1s infinite; }
  .status-dot.speaking  { background: var(--accent2); }

  @keyframes blink {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.4; }
  }

  .status-label { font-size: 13px; color: var(--text-muted); }
  .word-count   { font-size: 12px; color: var(--text-muted); margin-left: auto; }

  .btn-record {
    padding: 6px 18px;
    border-radius: 20px;
    border: 2px solid var(--accent);
    background: transparent;
    color: var(--accent);
    font-weight: 600;
    font-size: 13px;
    transition: all 0.15s;
  }
  .btn-record.active { background: var(--accent); color: #fff; }
  .btn-record:hover { opacity: 0.85; }

  .settings-body {
    display: flex;
    flex: 1;
    overflow: hidden;
  }

  .tab-nav {
    display: flex;
    flex-direction: column;
    width: 130px;
    flex-shrink: 0;
    border-right: 1px solid var(--border);
    padding: 12px 0;
    background: var(--surface);
    gap: 2px;
  }

  .tab-btn {
    display: block;
    width: 100%;
    text-align: left;
    padding: 9px 16px;
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 13px;
    border-radius: 0;
    transition: all 0.1s;
  }
  .tab-btn:hover { color: var(--text); background: var(--border); }
  .tab-btn.active { color: var(--accent2); background: rgba(79, 195, 247, 0.08); }

  .tab-content {
    flex: 1;
    overflow-y: auto;
    padding: 24px;
  }

  .settings-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 12px;
    padding: 10px 20px;
    border-top: 1px solid var(--border);
    background: var(--surface);
    flex-shrink: 0;
  }

  .unsaved { font-size: 12px; color: var(--text-muted); }

  .btn-save {
    padding: 6px 20px;
    border-radius: var(--radius);
    border: none;
    background: var(--accent);
    color: #fff;
    font-weight: 600;
    font-size: 13px;
    transition: opacity 0.15s;
  }
  .btn-save:disabled { opacity: 0.4; cursor: not-allowed; }
  .btn-save:not(:disabled):hover { opacity: 0.85; }
</style>
