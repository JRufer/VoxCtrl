<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OutputTarget, HotkeyBinding } from "./routing-types";

  let targets = $state<OutputTarget[]>([]);
  let bindings = $state<HotkeyBinding[]>([]);
  let saving = $state(false);
  let activeSection = $state<"targets" | "bindings">("targets");

  // Modals state
  let editingTarget = $state<OutputTarget | null>(null);
  let isEditingTargetNew = $state(false);
  let editingBinding = $state<HotkeyBinding | null>(null);
  let isEditingBindingNew = $state(false);
  let isRecordingKeys = $state(false);

  // Flat edit states to ensure absolute Svelte 5 reactivity for target processing overrides
  let editApplySnippets = $state(true);
  let editOllamaEnabled = $state(false);
  let editOllamaModel = $state("");
  let editOllamaMode = $state("custom");
  let editOllamaPrompt = $state("");
  let editMcpArgsString = $state("");

  // Derived JSON error validation for MCP custom arguments
  let mcpArgsError = $derived.by(() => {
    if (!editMcpArgsString.trim()) return null;
    try {
      JSON.parse(editMcpArgsString);
      return null;
    } catch (e: any) {
      return e.message;
    }
  });

  // Reusable Svelte action to auto-resize textareas dynamically to fit their contents
  function autoResize(node: HTMLTextAreaElement) {
    function resize() {
      node.style.height = "auto";
      node.style.height = `${node.scrollHeight}px`;
    }
    node.addEventListener("input", resize);
    // Initial calculation on mount or tab display
    const timer = setTimeout(resize, 0);

    return {
      update() {
        resize();
      },
      destroy() {
        clearTimeout(timer);
        node.removeEventListener("input", resize);
      }
    };
  }

  onMount(async () => {
    targets = await invoke<OutputTarget[]>("get_targets");
    bindings = await invoke<HotkeyBinding[]>("get_bindings");
  });

  async function persistTargets() {
    try {
      await invoke("save_targets", { targets });
      console.log("Targets auto-saved successfully!");
    } catch (e) {
      console.error("Failed to save targets:", e);
    }
  }

  async function persistBindings() {
    try {
      await invoke("save_bindings", { bindings });
      console.log("Bindings auto-saved successfully!");
    } catch (e) {
      console.error("Failed to save bindings:", e);
    }
  }

  // --- Helpers ---
  function deliveryLabel(d: string) {
    const map: Record<string, string> = {
      inject: "Inject Text directly", clipboard: "Copy to Clipboard", exec: "Execute Command",
      pipe: "Write to Named Pipe", socket: "Send to TCP/Unix Socket", file: "Write to File",
      dbus: "Emit DBus Signal", http: "HTTP Request", webhook: "Send Webhook",
      mcp: "Call MCP Server",
    };
    return map[d] ?? d;
  }

  function gestureLabel(g: string) {
    const map: Record<string, string> = {
      hold: "Hold Keys to Talk", toggle: "Tap to Start / Stop",
      double_tap: "Double-Tap to Record", chord: "Key Chord Combo",
    };
    return map[g] ?? g;
  }

  function formatBindingTargets(b: HotkeyBinding) {
    const ids = b.target_ids && b.target_ids.length > 0 ? b.target_ids : [b.target_id];
    return ids.map(id => {
      const t = targets.find(target => target.id === id);
      return t ? `${t.label} (${deliveryLabel(t.delivery)})` : (id === "default" ? "Focused Window" : id);
    }).join(" + ");
  }

  // --- CRUD Output Targets ---
  function addNewTarget() {
    isEditingTargetNew = true;
    editApplySnippets = true;
    editOllamaEnabled = false;
    editOllamaModel = "";
    editOllamaMode = "custom";
    editOllamaPrompt = "";
    editMcpArgsString = '{\n  "text": "{TEXT}"\n}';

    editingTarget = {
      id: "new_target_" + Math.random().toString(36).substring(2, 6),
      label: "New Target",
      delivery: "inject",
      file_prefix: "- ",
      file_timestamp: true,
      file_mode: "append",
      http_method: "POST",
      mcp_tool: "speak_text",
      mcp_args: { text: "{TEXT}" },
      send_on_release: false,
      append_newline: true,
      tts_engine: "None",
      processing: {
        apply_snippets: true,
        ollama_enabled: false,
        ollama_model: "",
        ollama_mode: "custom",
        ollama_prompt: "",
      }
    };
  }

  function editTarget(tgt: OutputTarget) {
    isEditingTargetNew = false;
    const clone = JSON.parse(JSON.stringify(tgt));
    if (!clone.processing) {
      clone.processing = {};
    }
    if (!clone.file_mode) {
      clone.file_mode = "append";
    }
    
    // Load flat state values
    editApplySnippets = clone.processing.apply_snippets !== false;
    editOllamaEnabled = clone.processing.ollama_enabled === true;
    editOllamaModel = clone.processing.ollama_model || "";
    editOllamaMode = clone.processing.ollama_mode || "custom";
    editOllamaPrompt = clone.processing.ollama_prompt || "";
    editMcpArgsString = clone.mcp_args ? JSON.stringify(clone.mcp_args, null, 2) : '{\n  "text": "{TEXT}"\n}';

    editingTarget = clone;
  }

  async function saveTargetModal() {
    if (!editingTarget) return;
    if (editingTarget.id.trim() === "") {
      alert("Target ID cannot be empty.");
      return;
    }

    // Force prompt template to have `{text}` if Ollama processing is enabled with custom mode
    if (editOllamaEnabled) {
      if (editOllamaMode === "custom") {
        if (!editOllamaPrompt.includes("{text}")) {
          alert("Ollama Configuration Error:\nYour custom prompt template MUST contain the '{text}' placeholder so Ollama knows where to insert the transcribed text.\n\nExample:\nwrite a hyku about {text}");
          return;
        }
      }
    }

    // Commit flat states back to the target's processing config before saving
    editingTarget.processing = {
      apply_snippets: editApplySnippets,
      ollama_enabled: editOllamaEnabled,
      ollama_model: editOllamaModel,
      ollama_mode: editOllamaMode,
      ollama_prompt: editOllamaPrompt,
    };
 
    if (editingTarget.delivery === "mcp") {
      try {
        editingTarget.mcp_args = JSON.parse(editMcpArgsString);
      } catch (e) {
        alert("Invalid JSON format in MCP Custom Tool Arguments.");
        return;
      }
    }

    if (isEditingTargetNew) {
      if (targets.some(t => t.id === editingTarget!.id)) {
        alert("Target with this ID already exists.");
        return;
      }
      targets = [...targets, editingTarget];
    } else {
      targets = targets.map(t => t.id === editingTarget!.id ? editingTarget! : t);
    }
    editingTarget = null;
    await persistTargets();
  }

  async function deleteTarget(id: string) {
    const usedBy = bindings.filter(b => {
      const ids = b.target_ids && b.target_ids.length > 0 ? b.target_ids : [b.target_id];
      return ids.includes(id);
    }).map(b => b.label || b.id);
    if (usedBy.length > 0) {
      alert(`Cannot delete target. It is currently being used by hotkeys: ${usedBy.join(", ")}`);
      return;
    }
    if (confirm("Are you sure you want to delete this target?")) {
      targets = targets.filter(t => t.id !== id);
      await persistTargets();
    }
  }

  // --- CRUD Hotkey Bindings ---
  function addNewBinding() {
    if (targets.length === 0) {
      alert("Please create at least one Output Target before making a hotkey binding.");
      return;
    }
    isEditingBindingNew = true;
    editingBinding = {
      id: "binding_" + Math.random().toString(36).substring(2, 6),
      label: "New Binding",
      keys: ["KEY_LEFTMETA", "KEY_SPACE"],
      gesture: "hold",
      target_id: targets[0].id,
      target_ids: [targets[0].id],
      tap_ms: 300,
      hold_threshold_ms: 500,
      disabled: false,
    };
  }

  function editBinding(b: HotkeyBinding) {
    isEditingBindingNew = false;
    const clone = JSON.parse(JSON.stringify(b));
    if (!clone.target_ids) {
      clone.target_ids = clone.target_id ? [clone.target_id] : [];
    }
    editingBinding = clone;
  }

  async function toggleBindingDisabled(id: string) {
    bindings = bindings.map(b => b.id === id ? { ...b, disabled: !b.disabled } : b);
    await persistBindings();
  }

  function addBindingTarget() {
    if (!editingBinding) return;
    if (!editingBinding.target_ids) {
      editingBinding.target_ids = [editingBinding.target_id];
    }
    // Find first target ID that is not already selected to avoid automatic duplicates
    const available = targets.find(t => !editingBinding!.target_ids!.includes(t.id));
    const nextId = available ? available.id : targets[0].id;
    editingBinding.target_ids = [...editingBinding.target_ids, nextId];
  }

  function removeBindingTarget(index: number) {
    if (!editingBinding || !editingBinding.target_ids) return;
    editingBinding.target_ids = editingBinding.target_ids.filter((_, i) => i !== index);
  }

  async function saveBindingModal() {
    if (!editingBinding) return;
    if (editingBinding.id.trim() === "") {
      alert("Binding ID cannot be empty.");
      return;
    }
    if (editingBinding.keys.length === 0) {
      alert("Please capture at least one hotkey before saving.");
      return;
    }
    if (editingBinding.target_ids && editingBinding.target_ids.length > 0) {
      // Clean up empty entries
      editingBinding.target_ids = editingBinding.target_ids.filter(id => id.trim() !== "");
      if (editingBinding.target_ids.length === 0) {
        alert("Please assign at least one Output Target.");
        return;
      }
      // Check for duplicates
      const uniqueIds = new Set(editingBinding.target_ids);
      if (uniqueIds.size !== editingBinding.target_ids.length) {
        alert("Duplicate targets detected.\nYou cannot assign the same Output Target multiple times to a single keybind.");
        return;
      }
      editingBinding.target_id = editingBinding.target_ids[0];
    } else {
      editingBinding.target_id = "";
      editingBinding.target_ids = [];
    }

    if (isEditingBindingNew) {
      if (bindings.some(b => b.id === editingBinding!.id)) {
        alert("Binding with this ID already exists.");
        return;
      }
      bindings = [...bindings, editingBinding];
    } else {
      bindings = bindings.map(b => b.id === editingBinding!.id ? editingBinding! : b);
    }
    editingBinding = null;
    isRecordingKeys = false;
    await persistBindings();
  }

  async function deleteBinding(id: string) {
    if (confirm("Are you sure you want to delete this hotkey binding?")) {
      bindings = bindings.filter(b => b.id !== id);
      await persistBindings();
    }
  }

  // --- Keyboard Event Capture / Recorder ---
  function mapBrowserKeyToEvdev(key: string, code: string): string {
    const codeUpper = code.toUpperCase();
    if (key === "Control") return "KEY_LEFTCTRL";
    if (key === "Alt") return "KEY_LEFTALT";
    if (key === "Shift") return "KEY_LEFTSHIFT";
    if (key === "Meta" || key === "OS" || key === "Super") return "KEY_LEFTMETA";

    if (codeUpper === "SPACE") return "KEY_SPACE";
    if (codeUpper === "ENTER") return "KEY_ENTER";
    if (codeUpper === "ESCAPE" || codeUpper === "ESC") return "KEY_ESC";
    if (codeUpper === "TAB") return "KEY_TAB";
    if (codeUpper === "BACKSPACE") return "KEY_BACKSPACE";
    if (codeUpper === "DELETE") return "KEY_DELETE";

    if (codeUpper.startsWith("KEY")) return codeUpper;
    if (codeUpper.startsWith("DIGIT")) return `KEY_${codeUpper.replace("DIGIT", "")}`;
    if (codeUpper.startsWith("ARROW")) return `KEY_${codeUpper.replace("ARROW", "")}`;
    if (codeUpper.startsWith("F") && codeUpper.length > 1) return `KEY_${codeUpper}`;

    if (key.length === 1) return `KEY_${key.toUpperCase()}`;
    return `KEY_${codeUpper}`;
  }

  let currentlyPressedKeys = $state<string[]>([]);

  function handleRecordKeyDown(e: KeyboardEvent) {
    if (!isRecordingKeys || !editingBinding) return;
    e.preventDefault();
    e.stopPropagation();

    const evdevKey = mapBrowserKeyToEvdev(e.key, e.code);
    if (!currentlyPressedKeys.includes(evdevKey)) {
      currentlyPressedKeys = [...currentlyPressedKeys, evdevKey];
    }
  }

  function handleRecordKeyUp(e: KeyboardEvent) {
    if (!isRecordingKeys || !editingBinding) return;
    e.preventDefault();
    e.stopPropagation();

    if (currentlyPressedKeys.length > 0) {
      editingBinding.keys = [...currentlyPressedKeys];
    }
    currentlyPressedKeys = [];
    isRecordingKeys = false; // Finish capture on release
  }
</script>

<section class="routing-section">
  <!-- Dynamic Subtab Navigation -->
  <div class="sub-nav">
    <button
      class="sub-btn"
      class:active={activeSection === "targets"}
      onclick={() => activeSection = "targets"}
    >
      🎯 Output Targets ({targets.length})
    </button>
    <button
      class="sub-btn"
      class:active={activeSection === "bindings"}
      onclick={() => activeSection = "bindings"}
    >
      ⌨ Hotkey Bindings ({bindings.length})
    </button>
  </div>

  {#if activeSection === "targets"}
    <!-- Output Targets Page -->
    <div class="section-header">
      <div>
        <h3>Output Targets</h3>
        <p class="description">Define routing destinations where transcribed text is typed, copied, piped, or sent over network sockets.</p>
      </div>
      <button class="btn-action primary" onclick={addNewTarget}>
        ＋ Add New Target
      </button>
    </div>

    <div class="cards-grid">
      {#each targets as t}
        <div class="card glass">
          <div class="card-header">
            <h4>{t.label}</h4>
            <span class="badge delivery">{t.delivery}</span>
          </div>
          <div class="card-body">
            <div class="info-row">
              <span class="info-label">ID:</span>
              <code class="info-val">{t.id}</code>
            </div>
            {#if t.delivery === "exec"}
              <div class="info-row">
                <span class="info-label">Cmd:</span>
                <span class="info-val line-clamp">{t.command}</span>
              </div>
            {/if}
            {#if t.delivery === "file"}
              <div class="info-row">
                <span class="info-label">File:</span>
                <span class="info-val line-clamp">{t.file_path}</span>
              </div>
            {/if}
            {#if t.delivery === "socket"}
              <div class="info-row">
                <span class="info-label">Socket:</span>
                <span class="info-val">{t.socket_host}:{t.socket_port}</span>
              </div>
            {/if}
            {#if t.delivery === "http" || t.delivery === "webhook"}
              <div class="info-row">
                <span class="info-label">API:</span>
                <span class="info-val line-clamp">{t.http_url || t.webhook_url}</span>
              </div>
            {/if}
            {#if t.delivery === "mcp"}
              <div class="info-row">
                <span class="info-label">MCP Tool:</span>
                <span class="info-val">{t.mcp_tool || "speak_text"}</span>
              </div>
            {/if}
          </div>
          <div class="card-actions">
            <button class="btn-action small" onclick={() => editTarget(t)}>Edit</button>
            <button class="btn-action small danger" onclick={() => deleteTarget(t.id)}>Delete</button>
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <span class="empty-icon">🎯</span>
          <p>No output targets defined. Create one to route your transcription output!</p>
        </div>
      {/each}
    </div>
  {:else}
    <!-- Hotkey Bindings Page -->
    <div class="section-header">
      <div>
        <h3>Hotkey Bindings</h3>
        <p class="description">Bind physical keyboard combinations to your output targets. Each hotkey supports customizable triggers (double-taps, holding keys, etc.)</p>
      </div>
      <button class="btn-action primary" onclick={addNewBinding}>
        ＋ Add New Binding
      </button>
    </div>

    <div class="cards-grid">
      {#each bindings as b}
        <div class="card glass" class:disabled={b.disabled}>
          <div class="card-header">
            <h4>{b.label || b.id}</h4>
            <span class="badge gesture">{b.gesture}</span>
          </div>
          <div class="card-body">
            <div class="keys-display">
              {#each b.keys as k}
                <kbd>{k.replace("KEY_", "")}</kbd>
              {/each}
            </div>
            <div class="info-row">
              <span class="info-label">Target:</span>
              <span class="info-val font-semibold">{formatBindingTargets(b)}</span>
            </div>
            {#if b.gesture === "hold"}
              <div class="info-row">
                <span class="info-label">Hold Timing:</span>
                <span class="info-val">{b.hold_threshold_ms}ms</span>
              </div>
            {/if}
            {#if b.gesture === "double_tap"}
              <div class="info-row">
                <span class="info-label">Tap Interval:</span>
                <span class="info-val">{b.tap_ms}ms</span>
              </div>
            {/if}
          </div>
          <div class="card-actions">
            <button class="btn-action small" onclick={() => toggleBindingDisabled(b.id)}>
              {b.disabled ? "Enable" : "Disable"}
            </button>
            <button class="btn-action small" onclick={() => editBinding(b)}>Edit</button>
            <button class="btn-action small danger" onclick={() => deleteBinding(b.id)}>Delete</button>
          </div>
        </div>
      {:else}
        <div class="empty-state">
          <span class="empty-icon">⌨</span>
          <p>No keyboard bindings defined. Create a binding to link a physical hotkey to a routing target!</p>
        </div>
      {/each}
    </div>
  {/if}

</section>

<!-- ========================================== -->
<!-- MODAL: Output Target Editor                -->
<!-- ========================================== -->
{#if editingTarget}
  <div class="modal-backdrop">
    <div class="modal glass animate-fade-in">
      <div class="modal-header">
        <h3>{isEditingTargetNew ? "Create Target" : "Edit Target"}</h3>
        <button class="close-btn" onclick={() => editingTarget = null}>✕</button>
      </div>
      <div class="modal-body">
        <label class="field">
          <span>Target ID</span>
          <input
            type="text"
            bind:value={editingTarget.id}
            placeholder="e.g. obsidian_vault"
            disabled={!isEditingTargetNew}
          />
        </label>

        <label class="field">
          <span>Display Label</span>
          <input
            type="text"
            bind:value={editingTarget.label}
            placeholder="e.g. Type directly into Obsidian"
          />
        </label>

        <label class="field">
          <span>Delivery System</span>
          <select bind:value={editingTarget.delivery}>
            <option value="inject">Inject Text Directly (Simulate keyboard)</option>
            <option value="clipboard">Save to Clipboard</option>
            <option value="exec">Execute Command</option>
            <option value="file">Write to File</option>
            <option value="socket">TCP / Unix Socket</option>
            <option value="dbus">DBus Signal</option>
            <option value="http">HTTP Custom Client</option>
            <option value="webhook">Send Webhook Event</option>
            <option value="mcp">Call MCP Server Tool</option>
          </select>
        </label>

        <!-- Dynamic morphing options based on delivery type -->
        {#if editingTarget.delivery === "exec"}
          <div class="morph-section mcp-container">
            <h5>Shell Executor Settings</h5>
            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Terminal Command</span>
                <span class="field-tag">Shell command</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.command}
                placeholder="e.g. xdg-open {'{TEXT}'}"
                class="full-width-input"
              />
              <p class="hint">Use <code>{"{TEXT}"}</code> inside the command string as a placeholder to substitute transcribed speech.</p>
            </div>
          </div>
        {/if}

        {#if editingTarget.delivery === "file"}
          <div class="morph-section">
            <h5>Local File Writer Settings</h5>
            <label class="field">
              <span>File Absolute Path</span>
              <input
                type="text"
                bind:value={editingTarget.file_path}
                placeholder="e.g. /home/user/notes.md"
              />
            </label>
            <label class="field">
              <span>Write Mode</span>
              <select bind:value={editingTarget.file_mode}>
                <option value="append">Append (Add to end of file)</option>
                <option value="prepend">Prepend (Add to beginning of file)</option>
              </select>
            </label>
            <label class="field">
              <span>Text Prefix</span>
              <input
                type="text"
                bind:value={editingTarget.file_prefix}
                placeholder="e.g. - "
              />
            </label>
            <label class="checkbox-field">
              <input type="checkbox" bind:checked={editingTarget.file_timestamp} />
              <span>Prepend date & time timestamp</span>
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "socket"}
          <div class="morph-section">
            <h5>Network Socket settings</h5>
            <label class="field">
              <span>Socket Host (TCP)</span>
              <input type="text" bind:value={editingTarget.socket_host} placeholder="localhost" />
            </label>
            <label class="field">
              <span>Socket Port (TCP)</span>
              <input type="number" bind:value={editingTarget.socket_port} placeholder="9000" />
            </label>
            <label class="field">
              <span>Unix Socket Path (Optional fallback)</span>
              <input type="text" bind:value={editingTarget.socket_unix} placeholder="/tmp/app.sock" />
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "dbus"}
          <div class="morph-section">
            <h5>Linux DBus emitter settings</h5>
            <label class="field">
              <span>Signal Method ID</span>
              <input type="text" bind:value={editingTarget.dbus_signal} placeholder="com.voxctl.TextReady" />
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "http" || editingTarget.delivery === "webhook"}
          <div class="morph-section">
            <h5>API Endpoint Client settings</h5>
            <label class="field">
              <span>REST endpoint URL</span>
              {#if editingTarget.delivery === 'http'}
                <input
                  type="text"
                  bind:value={editingTarget.http_url}
                  placeholder="https://api.example.com/transcribe"
                />
              {:else}
                <input
                  type="text"
                  bind:value={editingTarget.webhook_url}
                  placeholder="https://api.example.com/transcribe"
                />
              {/if}
            </label>
            {#if editingTarget.delivery === "http"}
              <label class="field">
                <span>Method</span>
                <select bind:value={editingTarget.http_method}>
                  <option value="POST">POST</option>
                  <option value="GET">GET</option>
                  <option value="PUT">PUT</option>
                </select>
              </label>
            {:else}
              <label class="field">
                <span>Webhook Secret Token (HMAC)</span>
                <input type="password" bind:value={editingTarget.webhook_secret} placeholder="Secret salt" />
              </label>
            {/if}
          </div>
        {/if}

        {#if editingTarget.delivery === "mcp"}
          <div class="morph-section mcp-container">
            <h5>MCP Server Client settings</h5>
            
            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">Custom Socket/Pipe Path</span>
                <span class="field-tag">Optional override</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.mcp_path}
                placeholder="e.g. /tmp/voxctl-mcp.sock"
                class="full-width-input"
              />
              <p class="hint">Leave empty to use defaults (<code>/tmp/voxctl-mcp.sock</code> on Linux, <code>\\.\pipe\voxctl-mcp</code> on Windows).</p>
            </div>

            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">MCP Tool Name</span>
              </div>
              <input
                type="text"
                bind:value={editingTarget.mcp_tool}
                placeholder="speak_text"
                class="full-width-input"
              />
            </div>

            <div class="field col mt-2">
              <div class="field-label-row">
                <span class="field-title">Custom Tool Arguments</span>
                <span class="field-tag">JSON Template</span>
              </div>
              <textarea
                rows="4"
                bind:value={editMcpArgsString}
                class:has-error={mcpArgsError}
                placeholder={'{"text": "{TEXT}"}'}
                use:autoResize
              ></textarea>
              <p class="hint">Must be valid JSON. Use <code>{"{TEXT}"}</code> to substitute transcribed speech.</p>
              {#if mcpArgsError}
                <span class="validation-error-msg">
                  ⚠️ Invalid JSON format: {mcpArgsError}
                </span>
              {/if}
            </div>
          </div>
        {/if}

        <!-- General Processing Toggles -->
        <div class="processing-toggles">
          <h5>Post-Processing & Output Tuning</h5>
          <label class="checkbox-field">
            <input type="checkbox" bind:checked={editingTarget.append_newline} />
            <span>Automatically append newline after transcribing</span>
          </label>
          <label class="checkbox-field">
            <input type="checkbox" bind:checked={editingTarget.send_on_release} />
            <span>Execute only on physical key release (Hold modes)</span>
          </label>

          {#if editingTarget.processing}
            <label class="checkbox-field">
              <input type="checkbox" bind:checked={editApplySnippets} />
              <span>Apply snippets to transcription text</span>
            </label>
            <label class="checkbox-field mt-2">
              <input type="checkbox" bind:checked={editOllamaEnabled} />
              <span>Enable target-specific Ollama LLM post-processing</span>
            </label>

            {#if editOllamaEnabled}
              <div class="ollama-target-settings pl-4 mt-2 ml-4">
                <label class="field">
                  <span>Model Override (leave empty for global default)</span>
                  <input
                    type="text"
                    bind:value={editOllamaModel}
                    placeholder="e.g. llama3.2:1b"
                  />
                </label>

                <div class="field col mt-2">
                  <div class="field-label-row">
                    <span class="field-title">Custom Prompt Template</span>
                    <span class="field-tag">LLM Prompt</span>
                  </div>
                  <textarea
                    rows="3"
                    bind:value={editOllamaPrompt}
                    class:has-error={editOllamaPrompt && !editOllamaPrompt.includes("{text}")}
                    placeholder="e.g. write a haiku about {'{text}'}"
                    use:autoResize
                  ></textarea>
                  {#if editOllamaPrompt && !editOllamaPrompt.includes("{text}")}
                    <span class="validation-error-msg">
                      ⚠️ Validation Error: Prompt template MUST contain the <code>{"{text}"}</code> placeholder.
                    </span>
                  {:else}
                    <p class="hint">Prompt template MUST contain the <code>{"{text}"}</code> placeholder.</p>
                  {/if}
                </div>
              </div>
            {/if}
          {/if}

          <label class="field mt-2">
            <span>System Prompt / Context (Optional)</span>
            <input
              type="text"
              bind:value={editingTarget.initial_prompt}
              placeholder="e.g. Format code variables in camelCase"
            />
          </label>

          <label class="field">
            <span>Voice Response Engine</span>
            <select bind:value={editingTarget.tts_engine}>
              <option value="None">Disabled (Silent Output)</option>
              <option value="Piper">Piper (Premium offline voice synthesis)</option>
              <option value="Espeak">eSpeak (Lightweight synthesizer)</option>
            </select>
          </label>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn-action secondary" onclick={() => editingTarget = null}>Cancel</button>
        <button class="btn-action primary" onclick={saveTargetModal}>Done</button>
      </div>
    </div>
  </div>
{/if}

<!-- ========================================== -->
<!-- MODAL: Hotkey Binding Editor               -->
<!-- ========================================== -->
{#if editingBinding}
  <div class="modal-backdrop">
    <div class="modal glass animate-fade-in">
      <div class="modal-header">
        <h3>{isEditingBindingNew ? "Create Hotkey Binding" : "Edit Hotkey Binding"}</h3>
        <button class="close-btn" onclick={() => { editingBinding = null; isRecordingKeys = false; }}>✕</button>
      </div>
      <div class="modal-body">
        <label class="field">
          <span>Binding Identifier</span>
          <input
            type="text"
            bind:value={editingBinding.id}
            placeholder="e.g. dictation_shortcut"
            disabled={!isEditingBindingNew}
          />
        </label>

        <label class="field">
          <span>Display Label</span>
          <input
            type="text"
            bind:value={editingBinding.label}
            placeholder="e.g. Global Speech to Text shortcut"
          />
        </label>

        <div class="field col">
          <div class="field-label-row">
            <span class="field-title">Assign to Output Targets</span>
            <button class="btn-add-inline" type="button" onclick={addBindingTarget}>
              ＋ Add Target
            </button>
          </div>

          <div class="target-selects-list">
            {#each editingBinding.target_ids || [] as tid, idx}
              <div class="target-select-row">
                <select bind:value={editingBinding.target_ids[idx]}>
                  {#each targets as t}
                    <option
                      value={t.id}
                      disabled={editingBinding.target_ids.includes(t.id) && t.id !== tid}
                    >
                      {t.label} ({t.delivery})
                    </option>
                  {/each}
                </select>
                {#if idx > 0}
                  <button
                    class="btn-remove-inline"
                    type="button"
                    onclick={() => removeBindingTarget(idx)}
                    title="Remove Target"
                  >
                    ✕
                  </button>
                {/if}
              </div>
            {/each}
          </div>
        </div>

        <label class="field">
          <span>Input Gesture Style</span>
          <select bind:value={editingBinding.gesture}>
            <option value="hold">Hold keys to dictate (Release to transcribe)</option>
            <option value="toggle">Tap once to start recording, tap again to finish</option>
            <option value="double_tap">Double-tap hotkey to trigger recording</option>
            <option value="chord">Chord combo (Held base keys + sub key)</option>
          </select>
        </label>

        <!-- Timings dynamic displays -->
        {#if editingBinding.gesture === "hold"}
          <label class="field morph-section">
            <span>Hold Threshold (ms)</span>
            <input type="number" bind:value={editingBinding.hold_threshold_ms} placeholder="500" />
            <span class="hint">Minimum duration to keep keys pressed to count as a 'hold'.</span>
          </label>
        {/if}

        {#if editingBinding.gesture === "double_tap"}
          <label class="field morph-section">
            <span>Double-Tap Interval (ms)</span>
            <input type="number" bind:value={editingBinding.tap_ms} placeholder="300" />
            <span class="hint">Maximum millisecond window between key presses to count as a double-tap.</span>
          </label>
        {/if}

        <!-- Premium Hotkey Recording Widget -->
        <div class="recorder-section">
          <h5>Hotkey Keybind Selection</h5>
          <div
            class="recorder-box"
            class:recording={isRecordingKeys}
            tabindex="0"
            role="button"
            aria-label="Hotkey recorder input"
            onclick={() => isRecordingKeys = true}
            onfocus={() => isRecordingKeys = true}
            onblur={() => isRecordingKeys = false}
            onkeydown={handleRecordKeyDown}
            onkeyup={handleRecordKeyUp}
          >
            {#if isRecordingKeys}
              <div class="pulse-container">
                <span class="pulse-dot"></span>
                <span class="recording-text">
                  {currentlyPressedKeys.length > 0
                    ? currentlyPressedKeys.join(" + ").replace(/KEY_/g, "")
                    : "Press your physical shortcut combination now..."}
                </span>
              </div>
            {:else}
              <span class="instruction-text">
                {#if editingBinding.keys.length > 0}
                  <div class="recorded-badges">
                    {#each editingBinding.keys as k}
                      <kbd class="kbd-recorder">{k.replace("KEY_", "")}</kbd>
                    {/each}
                  </div>
                  <span class="change-prompt">(Click / Tab here to record a new hotkey combo)</span>
                {:else}
                  ⚠️ Click/Focus here to press keys!
                {/if}
              </span>
            {/if}
          </div>
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn-action secondary" onclick={() => { editingBinding = null; isRecordingKeys = false; }}>Cancel</button>
        <button class="btn-action primary" onclick={saveBindingModal}>Done</button>
      </div>
    </div>
  </div>
{/if}

<style>
  @import "./tab.css";

  .routing-section {
    display: flex;
    flex-direction: column;
    gap: 20px;
    padding-bottom: 40px;
  }

  /* Sub-navigation tabs styling */
  .sub-nav {
    display: flex;
    gap: 8px;
    border-bottom: 1px solid var(--border);
    padding-bottom: 8px;
  }

  .sub-btn {
    background: transparent;
    border: none;
    padding: 8px 16px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted);
    border-radius: var(--radius);
    transition: all 0.15s ease-in-out;
  }

  .sub-btn:hover {
    color: var(--text);
    background: rgba(255, 255, 255, 0.04);
  }

  .sub-btn.active {
    color: var(--accent2);
    background: rgba(79, 195, 247, 0.08);
  }

  /* Header & Title bar */
  .section-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 20px;
    margin-bottom: 8px;
  }

  .description {
    font-size: 12px;
    color: var(--text-muted);
    margin: 4px 0 0 0;
  }

  /* Visual grid layout */
  .cards-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(280px, 1fr));
    gap: 16px;
  }

  /* Card and Glassmorphism design */
  .card {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 16px;
    gap: 12px;
    transition: all 0.2s ease-in-out;
    background: rgba(26, 31, 46, 0.4);
  }

  .card:hover {
    border-color: var(--accent2);
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.3);
    transform: translateY(-2px);
  }

  .card.disabled {
    opacity: 0.55;
    border-color: rgba(255, 255, 255, 0.05);
  }

  .card-header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 8px;
  }

  .card-header h4 {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
    color: var(--text);
  }

  .badge {
    font-size: 10px;
    padding: 2px 8px;
    border-radius: 12px;
    font-weight: 600;
    text-transform: uppercase;
  }

  .badge.delivery {
    background: rgba(124, 77, 255, 0.15);
    color: #b388ff;
    border: 1px solid rgba(124, 77, 255, 0.3);
  }

  .badge.gesture {
    background: rgba(0, 229, 255, 0.15);
    color: #84ffff;
    border: 1px solid rgba(0, 229, 255, 0.3);
  }

  .card-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }

  .info-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }

  .info-label {
    color: var(--text-muted);
  }

  .info-val {
    color: var(--text);
    text-align: right;
  }

  .line-clamp {
    display: -webkit-box;
    -webkit-line-clamp: 1;
    -webkit-box-orient: vertical;
    overflow: hidden;
    word-break: break-all;
  }

  .keys-display {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 4px;
  }

  kbd {
    background: var(--surface2);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 2px 6px;
    font-size: 11px;
    font-family: monospace;
    font-weight: 700;
    color: var(--accent2);
    box-shadow: 0 1px 0 rgba(0,0,0,0.4);
  }

  .card-actions {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    border-top: 1px solid rgba(255, 255, 255, 0.05);
    padding-top: 10px;
    margin-top: 4px;
  }

  /* Empty state */
  .empty-state {
    grid-column: 1 / -1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    padding: 40px;
    border: 2px dashed var(--border);
    border-radius: var(--radius);
    text-align: center;
    background: rgba(0, 0, 0, 0.15);
  }

  .empty-icon {
    font-size: 32px;
    margin-bottom: 8px;
  }

  .empty-state p {
    font-size: 13px;
    color: var(--text-muted);
    margin: 0;
  }

  /* Button Overrides */
  .btn-action {
    background: var(--surface2);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    padding: 6px 14px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-action:hover {
    background: var(--border);
    border-color: var(--text-muted);
  }

  .btn-action.primary {
    background: var(--accent);
    color: #fff;
    border: none;
  }

  .btn-action.primary:hover {
    opacity: 0.9;
  }

  .btn-action.small {
    padding: 4px 8px;
    font-size: 11px;
  }

  .btn-action.danger {
    color: #f87171;
    border-color: rgba(248, 113, 113, 0.2);
  }

  .btn-action.danger:hover {
    background: rgba(248, 113, 113, 0.1);
    border-color: #f87171;
  }

  /* Modal Dialog Glassmorphism style */
  .modal-backdrop {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    bottom: 0;
    background: rgba(0, 0, 0, 0.6);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 1000;
  }

  .modal {
    width: 90%;
    max-width: 500px;
    max-height: 85vh;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    display: flex;
    flex-direction: column;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.5);
    background: #1e2436;
  }

  .modal-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
  }

  .modal-header h3 {
    margin: 0;
    font-size: 15px;
    font-weight: 700;
  }

  .close-btn {
    background: transparent;
    border: none;
    color: var(--text-muted);
    font-size: 16px;
    cursor: pointer;
  }

  .close-btn:hover {
    color: var(--text);
  }

  .modal-body {
    padding: 20px;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .modal-footer {
    display: flex;
    justify-content: flex-end;
    gap: 10px;
    padding: 16px 20px;
    border-top: 1px solid var(--border);
  }

  /* Morph & Nested structures */
  .morph-section {
    background: rgba(255, 255, 255, 0.02);
    border: 1px dashed var(--border);
    border-radius: var(--radius);
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .morph-section h5, .processing-toggles h5, .recorder-section h5 {
    margin: 0 0 4px 0;
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    color: var(--accent2);
  }

  .processing-toggles {
    border-top: 1px solid var(--border);
    padding-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .checkbox-field {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    font-size: 12px;
    color: var(--text-muted);
  }

  .checkbox-field input[type="checkbox"] {
    cursor: pointer;
  }

  .checkbox-field:hover {
    color: var(--text);
  }

  /* Hotkey Recording Box */
  .recorder-section {
    border-top: 1px solid var(--border);
    padding-top: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .recorder-box {
    background: rgba(0, 0, 0, 0.25);
    border: 2px dashed var(--border);
    border-radius: var(--radius);
    padding: 24px;
    text-align: center;
    cursor: pointer;
    outline: none;
    transition: all 0.2s ease;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    min-height: 80px;
  }

  .recorder-box:hover, .recorder-box:focus {
    border-color: var(--accent2);
    background: rgba(0, 0, 0, 0.35);
  }

  .recorder-box.recording {
    border-color: var(--accent);
    background: rgba(244, 63, 94, 0.05);
    border-style: solid;
    animation: border-pulse 1.5s infinite;
  }

  @keyframes border-pulse {
    0%, 100% { border-color: rgba(244, 63, 94, 0.4); }
    50% { border-color: rgba(244, 63, 94, 1); }
  }

  .pulse-container {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .pulse-dot {
    width: 8px;
    height: 8px;
    background: var(--accent);
    border-radius: 50%;
    animation: flash 1s infinite;
  }

  @keyframes flash {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.3; }
  }

  .recording-text {
    font-size: 13px;
    font-weight: 600;
    color: var(--accent);
  }

  .instruction-text {
    font-size: 12px;
    color: var(--text-muted);
    display: flex;
    flex-direction: column;
    gap: 8px;
    align-items: center;
  }

  .recorded-badges {
    display: flex;
    gap: 6px;
  }

  .kbd-recorder {
    font-size: 12px;
    background: var(--accent2);
    color: #000;
    border: none;
    font-weight: 800;
  }

  .change-prompt {
    font-size: 10px;
    color: var(--accent2);
    opacity: 0.8;
  }

  .actions {
    display: flex;
    justify-content: flex-end;
    border-top: 1px solid var(--border);
    padding-top: 14px;
  }

  .btn-save {
    background: var(--accent);
    color: #fff;
    border: none;
    border-radius: var(--radius);
    padding: 8px 24px;
    font-weight: 600;
    font-size: 13px;
    cursor: pointer;
    transition: opacity 0.15s ease;
  }

  .btn-save:hover {
    opacity: 0.9;
  }

  .btn-save:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .font-semibold {
    font-weight: 600;
  }

  .mt-2 {
    margin-top: 8px;
  }

  /* Keyframe transitions for modals */
  .animate-fade-in {
    animation: fade-in 0.2s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes fade-in {
    from { opacity: 0; transform: scale(0.96); }
    to { opacity: 1; transform: scale(1); }
  }

  .ollama-target-settings {
    border-left: 2px solid var(--accent);
    padding-left: 14px;
    margin-left: 10px;
    margin-top: 10px;
    margin-bottom: 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .prompt-error-msg {
    color: #e57373;
    font-size: 12px;
    margin-top: 4px;
    font-weight: 500;
  }
  textarea {
    width: 100%;
    background: var(--color-obsidian-950);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    color: var(--text);
    padding: 8px 12px;
    font-size: 13px;
    font-family: monospace;
    resize: vertical;
    outline: none;
    box-shadow: inset 0 2px 4px rgba(0, 0, 0, 0.2);
    transition: var(--transition-spring-fast);
  }
  textarea:hover {
    border-color: rgba(255, 255, 255, 0.15);
    background-color: var(--color-obsidian-900);
  }
  textarea:focus {
    border-color: var(--accent2);
    background-color: var(--color-obsidian-950);
    box-shadow: 0 0 0 2px rgba(56, 189, 248, 0.15), inset 0 2px 4px rgba(0, 0, 0, 0.2);
  }
  .field.col {
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    width: 100%;
  }

  /* MCP & Ollama Redesigned Form Styles */
  .mcp-container {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 6px 0;
  }

  .field-label-row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    margin-bottom: 2px;
  }

  .field-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text);
  }

  .field-tag {
    font-size: 9px;
    padding: 2px 6px;
    background: rgba(79, 195, 247, 0.08);
    color: var(--accent2);
    border: 1px solid rgba(79, 195, 247, 0.25);
    border-radius: 4px;
    text-transform: uppercase;
    font-weight: 700;
    letter-spacing: 0.04em;
    line-height: 1;
  }

  .full-width-input {
    width: 100% !important;
    box-sizing: border-box;
  }

  p.hint {
    margin: 4px 0 0 0;
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.5;
  }

  p.hint code {
    background: var(--color-obsidian-950);
    color: var(--color-accent-blue);
    padding: 1px 4px;
    border-radius: 3px;
    font-family: 'JetBrains Mono', monospace;
    font-size: 10px;
    border: 1px solid var(--border);
  }

  /* Input validation visual status styles */
  textarea.has-error, input.has-error {
    border-color: #e57373 !important;
    box-shadow: 0 0 0 2px rgba(229, 115, 115, 0.15), inset 0 2px 4px rgba(0, 0, 0, 0.2) !important;
  }

  .validation-error-msg {
    display: block;
    margin-top: 4px;
    font-size: 11px;
    font-weight: 500;
    color: #e57373;
    line-height: 1.4;
  }

  .btn-add-inline {
    background: transparent;
    border: none;
    color: var(--accent2);
    font-size: 11px;
    font-weight: 750;
    cursor: pointer;
    padding: 2px 6px;
    border-radius: 4px;
    transition: all 0.15s ease;
  }
  .btn-add-inline:hover {
    background: rgba(79, 195, 247, 0.08);
    color: #fff;
  }

  .target-selects-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
    margin-top: 4px;
  }

  .target-select-row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
  }

  .target-select-row select {
    flex: 1;
    width: 100%;
  }

  .btn-remove-inline {
    background: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.2);
    color: #f87171;
    cursor: pointer;
    font-size: 12px;
    font-weight: 700;
    padding: 6px 12px;
    border-radius: var(--radius);
    transition: all 0.15s ease;
    display: flex;
    align-items: center;
    justify-content: center;
    height: 34px;
    box-sizing: border-box;
  }
  .btn-remove-inline:hover {
    background: #ef4444;
    border-color: #ef4444;
    color: #fff;
    box-shadow: 0 2px 8px rgba(239, 68, 68, 0.25);
  }
</style>
