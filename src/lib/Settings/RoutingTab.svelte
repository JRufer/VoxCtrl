<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { OutputTarget, HotkeyBinding } from "./routing-types";

  let targets = $state<OutputTarget[]>([]);
  let bindings = $state<HotkeyBinding[]>([]);
  let saving = $state(false);
  let activeSection = $state<"targets" | "bindings">("bindings");

  // Modals state
  let editingTarget = $state<OutputTarget | null>(null);
  let isEditingTargetNew = $state(false);
  let originalTargetId = $state<string | null>(null);
  let editingBinding = $state<HotkeyBinding | null>(null);
  let isEditingBindingNew = $state(false);
  let recordingTarget = $state<"keys" | "subkey" | null>(null);
  let confirmDeleteTargetId = $state<string | null>(null);
  let confirmDeleteBindingId = $state<string | null>(null);

  // Flat edit states to ensure absolute Svelte 5 reactivity for target processing overrides
  let editApplySnippets = $state(true);
  let editOllamaEnabled = $state(false);
  let editOllamaModel = $state("");
  let editOllamaMode = $state("custom");
  let editOllamaPrompt = $state("");
  let editMcpArgsString = $state("");
  let editHttpTemplateString = $state("");
  let editWebhookTemplateString = $state("");

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

  // Derived JSON error validation for HTTP custom template
  let httpTemplateError = $derived.by(() => {
    if (!editHttpTemplateString.trim()) return null;
    try {
      JSON.parse(editHttpTemplateString);
      return null;
    } catch (e: any) {
      return e.message;
    }
  });

  // Derived JSON error validation for Webhook custom template
  let webhookTemplateError = $derived.by(() => {
    if (!editWebhookTemplateString.trim()) return null;
    try {
      JSON.parse(editWebhookTemplateString);
      return null;
    } catch (e: any) {
      return e.message;
    }
  });

  // Helper to construct a canonical signature for a binding's key combination and gesture type
  function getBindingSignature(keys: string[], gesture: string, subkey?: string): string {
    const sortedKeys = [...keys].sort().join(",");
    const subkeyPart = gesture === "chord" && subkey ? `+${subkey}` : "";
    return `${gesture}:${sortedKeys}${subkeyPart}`;
  }

  // Derived set of signatures that are shared by two or more bindings (regardless of disabled state)
  let conflictingSignatures = $derived.by(() => {
    const sigCounts = new Map<string, number>();
    for (const b of bindings) {
      if (b.keys && b.keys.length > 0) {
        const sig = getBindingSignature(b.keys, b.gesture, b.subkey);
        sigCounts.set(sig, (sigCounts.get(sig) || 0) + 1);
      }
    }
    const dups = new Set<string>();
    for (const [sig, count] of sigCounts.entries()) {
      if (count > 1) {
        dups.add(sig);
      }
    }
    return dups;
  });

  // Derived set of signatures that are shared by two or more active/enabled bindings
  let activeConflictingSignatures = $derived.by(() => {
    const activeSigCounts = new Map<string, number>();
    for (const b of bindings) {
      if (!b.disabled && b.keys && b.keys.length > 0) {
        const sig = getBindingSignature(b.keys, b.gesture, b.subkey);
        activeSigCounts.set(sig, (activeSigCounts.get(sig) || 0) + 1);
      }
    }
    const dups = new Set<string>();
    for (const [sig, count] of activeSigCounts.entries()) {
      if (count > 1) {
        dups.add(sig);
      }
    }
    return dups;
  });

  // Derived check: does the current edited binding in the modal conflict with another existing binding?
  let editingBindingConflict = $derived.by(() => {
    if (!editingBinding || !editingBinding.keys || editingBinding.keys.length === 0) return false;
    const editSig = getBindingSignature(editingBinding.keys, editingBinding.gesture, editingBinding.subkey);
    return bindings.some(b => b.id !== editingBinding!.id && getBindingSignature(b.keys, b.gesture, b.subkey) === editSig);
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
      double_tap: "Double-Tap to Record", double_tap_hold: "Double-Tap & Hold to Record",
      chord: "Key Chord Combo",
    };
    return map[g] ?? g;
  }

  function formatBindingTargets(b: HotkeyBinding) {
    const ids = b.target_ids && b.target_ids.length > 0 ? b.target_ids : [b.target_id];
    return ids.map(id => {
      const t = targets.find(target => target.id === id);
      return t ? t.label : (id === "default" ? "Focused Window" : id);
    }).join(", ");
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
    editHttpTemplateString = '{\n  "text": "{TEXT}"\n}';
    editWebhookTemplateString = '{\n  "text": "{TEXT}"\n}';

    editingTarget = {
      id: "new_target_" + Math.random().toString(36).substring(2, 6),
      label: "New Target",
      delivery: "inject",
      file_prefix: "- ",
      file_timestamp: true,
      file_mode: "append",
      http_method: "POST",
      http_json_template: { text: "{TEXT}" },
      webhook_json_template: { text: "{TEXT}" },
      mcp_tool: "speak_text",
      mcp_args: { text: "{TEXT}" },
      send_on_release: false,
      append_newline: true,
      strip_newlines: false,
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
    originalTargetId = tgt.id;
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
    editHttpTemplateString = clone.http_json_template ? JSON.stringify(clone.http_json_template, null, 2) : '{\n  "text": "{TEXT}"\n}';
    editWebhookTemplateString = clone.webhook_json_template ? JSON.stringify(clone.webhook_json_template, null, 2) : '{\n  "text": "{TEXT}"\n}';

    editingTarget = clone;
  }

  async function saveTargetModal() {
    if (!editingTarget) return;
    if (editingTarget.id.trim() === "") {
      alert("Target ID cannot be empty.");
      return;
    }

    // Commit flat states back to the target's processing config before saving
    editingTarget.processing = {
      apply_snippets: editApplySnippets,
    };
 
    if (editingTarget.delivery === "mcp") {
      try {
        editingTarget.mcp_args = JSON.parse(editMcpArgsString);
      } catch (e) {
        alert("Invalid JSON format in MCP Custom Tool Arguments.");
        return;
      }
    }

    if (editingTarget.delivery === "http") {
      try {
        editingTarget.http_json_template = JSON.parse(editHttpTemplateString);
      } catch (e) {
        alert("Invalid JSON format in HTTP Custom Post Body.");
        return;
      }
    }

    if (editingTarget.delivery === "webhook") {
      try {
        editingTarget.webhook_json_template = JSON.parse(editWebhookTemplateString);
      } catch (e) {
        alert("Invalid JSON format in Webhook Custom Post Body.");
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
      if (originalTargetId !== editingTarget!.id && targets.some(t => t.id === editingTarget!.id)) {
        alert("Another target with this ID already exists.");
        return;
      }

      // Update bindings that point to the old ID
      if (originalTargetId && originalTargetId !== editingTarget!.id) {
        let bindingsChanged = false;
        bindings = bindings.map(b => {
          let updated = false;
          let newTargetIds = b.target_ids ? [...b.target_ids] : [b.target_id];
          for (let i = 0; i < newTargetIds.length; i++) {
            if (newTargetIds[i] === originalTargetId) {
              newTargetIds[i] = editingTarget!.id;
              updated = true;
            }
          }
          if (updated) {
            bindingsChanged = true;
            return { ...b, target_ids: newTargetIds, target_id: newTargetIds[0] };
          }
          return b;
        });

        if (bindingsChanged) {
          await persistBindings();
        }
      }

      targets = targets.map(t => t.id === originalTargetId ? editingTarget! : t);
    }
    editingTarget = null;
    originalTargetId = null;
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
    targets = targets.filter(t => t.id !== id);
    await persistTargets();
  }

  // --- CRUD Hotkey Bindings ---
  function addNewBinding() {
    if (targets.length === 0) {
      alert("Please create at least one Output Target before making a hotkey binding.");
      return;
    }
    isEditingBindingNew = true;
    editOllamaEnabled = false;
    editOllamaModel = "";
    editOllamaMode = "custom";
    editOllamaPrompt = "";
    editingBinding = {
      id: "binding_" + Math.random().toString(36).substring(2, 6),
      label: "New Binding",
      keys: ["KEY_LEFTMETA", "KEY_SPACE"],
      gesture: "hold",
      target_id: targets[0].id,
      target_ids: [targets[0].id],
      tap_ms: 300,
      hold_threshold_ms: 1000,
      subkey: "",
      disabled: false,
      ollama_enabled: false,
      ollama_model: "",
      ollama_mode: "custom",
      ollama_prompt: "",
    };
  }

  function editBinding(b: HotkeyBinding) {
    isEditingBindingNew = false;
    const clone = JSON.parse(JSON.stringify(b));
    if (!clone.target_ids) {
      clone.target_ids = clone.target_id ? [clone.target_id] : [];
    }
    editOllamaEnabled = clone.ollama_enabled === true;
    editOllamaModel = clone.ollama_model || "";
    editOllamaMode = clone.ollama_mode || "custom";
    editOllamaPrompt = clone.ollama_prompt || "";
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
    if (editingBinding.gesture === "chord" && (!editingBinding.subkey || editingBinding.subkey.trim() === "")) {
      alert("Please capture a subkey trigger for the chord combo before saving.");
      return;
    }

    // Force prompt template to have `{text}` if Ollama processing is enabled with custom mode
    if (editOllamaEnabled) {
      if (editOllamaMode === "custom") {
        if (!editOllamaPrompt.includes("{text}")) {
          alert("Ollama Configuration Error:\nYour custom prompt template MUST contain the '{text}' placeholder so Ollama knows where to insert the transcribed text.\n\nExample:\nwrite a haiku about {text}");
          return;
        }
      }
    }

    editingBinding.ollama_enabled = editOllamaEnabled;
    editingBinding.ollama_model = editOllamaModel;
    editingBinding.ollama_mode = editOllamaMode;
    editingBinding.ollama_prompt = editOllamaPrompt;
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
    recordingTarget = null;
    await persistBindings();
  }

  async function deleteBinding(id: string) {
    bindings = bindings.filter(b => b.id !== id);
    await persistBindings();
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

    if (/^KEY[A-Z]$/.test(codeUpper)) {
      return `KEY_${codeUpper.slice(3)}`;
    }
    if (codeUpper.startsWith("KEY")) return codeUpper;
    if (codeUpper.startsWith("DIGIT")) return `KEY_${codeUpper.replace("DIGIT", "")}`;
    if (codeUpper.startsWith("ARROW")) return `KEY_${codeUpper.replace("ARROW", "")}`;
    if (codeUpper.startsWith("F") && codeUpper.length > 1) return `KEY_${codeUpper}`;

    if (key.length === 1) return `KEY_${key.toUpperCase()}`;
    return `KEY_${codeUpper}`;
  }

  let currentlyPressedKeys = $state<string[]>([]);

  function handleRecordKeyDown(e: KeyboardEvent) {
    if (!recordingTarget || !editingBinding) return;
    e.preventDefault();
    e.stopPropagation();

    const evdevKey = mapBrowserKeyToEvdev(e.key, e.code);
    if (recordingTarget === "keys") {
      if (!currentlyPressedKeys.includes(evdevKey)) {
        currentlyPressedKeys = [...currentlyPressedKeys, evdevKey];
      }
      // Escape triggers browser blur before keyup fires, so commit immediately
      if (e.key === "Escape") {
        editingBinding.keys = [...currentlyPressedKeys];
        currentlyPressedKeys = [];
        recordingTarget = null;
      }
    } else if (recordingTarget === "subkey") {
      editingBinding.subkey = evdevKey;
      recordingTarget = null;
    }
  }

  function handleRecordKeyUp(e: KeyboardEvent) {
    if (!recordingTarget || !editingBinding) return;
    e.preventDefault();
    e.stopPropagation();

    if (recordingTarget === "keys") {
      if (currentlyPressedKeys.length > 0) {
        editingBinding.keys = [...currentlyPressedKeys];
      }
      currentlyPressedKeys = [];
      recordingTarget = null; // Finish capture on release
    }
  }

  function handleRecordKeyBlur() {
    if (!recordingTarget) return;
    if (recordingTarget === "keys" && currentlyPressedKeys.length > 0 && editingBinding) {
      editingBinding.keys = [...currentlyPressedKeys];
      currentlyPressedKeys = [];
    }
    recordingTarget = null;
  }
</script>

<section class="routing-section">
  <!-- Dynamic Subtab Navigation -->
  <div class="sub-nav">
    <button
      class="sub-btn"
      class:active={activeSection === "bindings"}
      onclick={() => activeSection = "bindings"}
    >
      ⌨ Hotkey Bindings ({bindings.length})
    </button>
    <button
      class="sub-btn"
      class:active={activeSection === "targets"}
      onclick={() => activeSection = "targets"}
    >
      🎯 Output Targets ({targets.length})
    </button>
  </div>

  {#if activeSection === "targets"}
    <!-- Output Targets Page -->
    <div class="section-header">
      <div>
        <h3>Output Targets</h3>
        <p class="description">Define routing destinations where transcribed text is typed, copied, piped, or sent over network sockets.</p>
      </div>
    </div>

    <button class="btn-add-wide" onclick={addNewTarget}>
      ＋ Add New Output Target
    </button>

    <div class="bindings-list">
      {#each targets as t}
        <div class="binding-item glass">
          <div class="binding-content">
            <div class="binding-row2">
              <div class="binding-title">{t.label}</div>
              <span class="badge delivery">{t.delivery}</span>
            </div>
            {#if t.delivery === "exec"}
              <div class="binding-targets">Cmd: {t.command}</div>
            {/if}
            {#if t.delivery === "file"}
              <div class="binding-targets">File: {t.file_path}</div>
            {/if}
            {#if t.delivery === "socket"}
              <div class="binding-targets">Socket: {t.socket_host}:{t.socket_port}</div>
            {/if}
            {#if t.delivery === "http" || t.delivery === "webhook"}
              <div class="binding-targets">API: {t.http_url || t.webhook_url}</div>
            {/if}
            {#if t.delivery === "mcp"}
              <div class="binding-targets">MCP Tool: {t.mcp_tool || "speak_text"}</div>
            {/if}
            <div class="binding-actions">
              <button class="btn-action small" onclick={() => editTarget(t)}>Edit</button>
              {#if confirmDeleteTargetId === t.id}
                <span class="confirm-label">Delete?</span>
                <button class="btn-action small danger" onclick={() => { deleteTarget(t.id); confirmDeleteTargetId = null; }}>Yes</button>
                <button class="btn-action small" onclick={() => confirmDeleteTargetId = null}>No</button>
              {:else}
                <button class="btn-action small danger" onclick={() => confirmDeleteTargetId = t.id}>Delete</button>
              {/if}
            </div>
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
    </div>

    <button class="btn-add-wide" onclick={addNewBinding}>
      ＋ Add New Hotkey Binding
    </button>

    {#if conflictingSignatures.size > 0}
      <div class="conflict-alert-banner animate-fade-in">
        <span class="warning-icon">⚠️</span>
        <span class="alert-message">
          <strong>Conflict detected:</strong> Some key bindings share the same key combination and gesture type. Disabling one of the conflicting binds will fix the problem.
        </span>
      </div>
    {/if}

    <div class="bindings-list">
      {#each bindings as b}
        <div
          class="binding-item glass"
          class:disabled={b.disabled}
          class:has-conflict={b.keys && b.keys.length > 0 && conflictingSignatures.has(getBindingSignature(b.keys, b.gesture, b.subkey))}
          class:active-conflict={!b.disabled && b.keys && b.keys.length > 0 && activeConflictingSignatures.has(getBindingSignature(b.keys, b.gesture, b.subkey))}
        >
          {#if !b.disabled && b.keys && b.keys.length > 0 && activeConflictingSignatures.has(getBindingSignature(b.keys, b.gesture, b.subkey))}
            <span class="conflict-marker">CONFLICT</span>
          {/if}
          <div class="binding-content">
            <div class="binding-header-row">
              <div
                class="binding-title"
                class:has-conflict={!b.disabled && b.keys && b.keys.length > 0 && activeConflictingSignatures.has(getBindingSignature(b.keys, b.gesture, b.subkey))}
              >
                {b.label || b.id}
              </div>
              {#if b.ollama_enabled}
                <span class="badge ollama">Ollama LLM</span>
              {/if}
            </div>
            <div class="binding-row2">
              <div class="keys-display">
                {#each b.keys as k}
                  <kbd>{k.replace("KEY_", "")}</kbd>
                {/each}
                {#if b.gesture === "chord" && b.subkey}
                  <span style="color: rgba(255, 255, 255, 0.4); font-weight: bold; margin: 0 4px; align-self: center;">＋</span>
                  <kbd style="border-color: var(--accent2); color: var(--accent2); font-weight: bold;">{b.subkey.replace("KEY_", "")}</kbd>
                {/if}
              </div>
              <span class="badge gesture">{b.gesture}</span>
            </div>
            <div class="binding-targets">{formatBindingTargets(b)}</div>
            <div class="binding-actions">
              <button class="btn-action small" onclick={() => toggleBindingDisabled(b.id)}>
                {b.disabled ? "Enable" : "Disable"}
              </button>
              <button class="btn-action small" onclick={() => editBinding(b)}>Edit</button>
              {#if confirmDeleteBindingId === b.id}
                <span class="confirm-label">Delete?</span>
                <button class="btn-action small danger" onclick={() => { deleteBinding(b.id); confirmDeleteBindingId = null; }}>Yes</button>
                <button class="btn-action small" onclick={() => confirmDeleteBindingId = null}>No</button>
              {:else}
                <button class="btn-action small danger" onclick={() => confirmDeleteBindingId = b.id}>Delete</button>
              {/if}
            </div>
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
            class="longer-display-label"
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
              <input type="text" bind:value={editingTarget.dbus_signal} placeholder="com.voxctrl.TextReady" />
            </label>
          </div>
        {/if}

        {#if editingTarget.delivery === "http" || editingTarget.delivery === "webhook"}
          <div class="morph-section mcp-container">
            <h5>API Endpoint Client settings</h5>
            <div class="field col">
              <div class="field-label-row">
                <span class="field-title">REST endpoint URL</span>
              </div>
              {#if editingTarget.delivery === 'http'}
                <input
                  type="text"
                  bind:value={editingTarget.http_url}
                  placeholder="https://api.example.com/transcribe"
                  class="full-width-input"
                />
              {:else}
                <input
                  type="text"
                  bind:value={editingTarget.webhook_url}
                  placeholder="https://api.example.com/transcribe"
                  class="full-width-input"
                />
              {/if}
            </div>

            {#if editingTarget.delivery === "http"}
              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Method</span>
                </div>
                <select bind:value={editingTarget.http_method} style="width: 100%;">
                  <option value="POST">POST</option>
                  <option value="GET">GET</option>
                  <option value="PUT">PUT</option>
                </select>
              </div>

              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Custom Post Body</span>
                  <span class="field-tag">JSON Template</span>
                </div>
                <textarea
                  rows="4"
                  bind:value={editHttpTemplateString}
                  class:has-error={httpTemplateError}
                  placeholder={'{"text": "{TEXT}"}'}
                  use:autoResize
                ></textarea>
                <p class="hint">Must be valid JSON. Use <code>{"{TEXT}"}</code> to substitute transcribed speech.</p>
                {#if httpTemplateError}
                  <span class="validation-error-msg">
                    ⚠️ Invalid JSON format: {httpTemplateError}
                  </span>
                {/if}
              </div>
            {:else}
              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Webhook Secret Token (HMAC)</span>
                </div>
                <input type="password" bind:value={editingTarget.webhook_secret} placeholder="Secret salt" class="full-width-input" />
              </div>

              <div class="field col mt-2">
                <div class="field-label-row">
                  <span class="field-title">Custom Webhook Body</span>
                  <span class="field-tag">JSON Template</span>
                </div>
                <textarea
                  rows="4"
                  bind:value={editWebhookTemplateString}
                  class:has-error={webhookTemplateError}
                  placeholder={'{"text": "{TEXT}"}'}
                  use:autoResize
                ></textarea>
                <p class="hint">Must be valid JSON. Use <code>{"{TEXT}"}</code> to substitute transcribed speech.</p>
                {#if webhookTemplateError}
                  <span class="validation-error-msg">
                    ⚠️ Invalid JSON format: {webhookTemplateError}
                  </span>
                {/if}
              </div>
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
                placeholder="e.g. /tmp/voxctrl-mcp.sock"
                class="full-width-input"
              />
              <p class="hint">Leave empty to use defaults (<code>/tmp/voxctrl-mcp.sock</code> on Linux, <code>\\.\pipe\voxctrl-mcp</code> on Windows).</p>
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
          {#if editingTarget.delivery === "inject"}
            <label class="checkbox-field">
              <input type="checkbox" bind:checked={editingTarget.strip_newlines} />
              <span>Strip newlines and carriage returns (Single-line mode)</span>
            </label>
          {/if}

          {#if editingTarget.processing}
            <label class="checkbox-field">
              <input type="checkbox" bind:checked={editApplySnippets} />
              <span>Apply snippets to transcription text</span>
            </label>
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
        <button class="close-btn" onclick={() => { editingBinding = null; recordingTarget = null; }}>✕</button>
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
            class="longer-display-label"
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
            <option value="double_tap_hold">Double-tap & hold keys to dictate (Release to transcribe)</option>
            <option value="chord">Chord combo (Held base keys + sub key)</option>
          </select>
        </label>

        <!-- Timings dynamic displays -->
        {#if editingBinding.gesture === "hold" || editingBinding.gesture === "double_tap_hold"}
          <label class="field morph-section">
            <span>Hold Threshold (ms)</span>
            <input type="number" bind:value={editingBinding.hold_threshold_ms} placeholder="1000" />
            <span class="hint">Minimum duration to keep keys pressed to count as a 'hold'.</span>
          </label>
        {/if}

        {#if editingBinding.gesture === "double_tap" || editingBinding.gesture === "double_tap_hold"}
          <label class="field morph-section">
            <span>Double-Tap Interval (ms)</span>
            <input type="number" bind:value={editingBinding.tap_ms} placeholder="300" />
            <span class="hint">Maximum millisecond window between key presses to count as a double-tap.</span>
          </label>
        {/if}

        <!-- Premium Hotkey Recording Widget -->
        <div class="border-t border-white/5 pt-[14px] flex flex-col gap-3">
          <div class="flex flex-col gap-1.5">
            <h5 class="text-[11px] font-bold uppercase text-accent-blue tracking-[0.06em]">
              {editingBinding.gesture === "chord" ? "Base Combo (Held Keys)" : "Hotkey Keybind Selection"}
            </h5>
            <div
              class={[
                "border-2 rounded-desktop p-4 text-center cursor-pointer outline-none transition-all duration-200 flex flex-col items-center justify-center min-h-[70px]",
                recordingTarget === "keys"
                  ? "border-solid border-[#f43f5e] bg-[rgba(244,63,94,0.05)] animate-border-pulse"
                  : (editingBindingConflict
                    ? "border-solid border-[#fbbf24] bg-[rgba(251,191,36,0.05)] hover:border-[#fbbf24]/80"
                    : "border-dashed border-white/5 bg-black/25 hover:border-accent-blue hover:bg-black/35 focus:border-accent-blue focus:bg-black/35")
              ].join(" ")}
              tabindex="0"
              role="button"
              aria-label="Base Hotkey recorder input"
              onclick={() => recordingTarget = "keys"}
              onfocus={() => recordingTarget = "keys"}
              onblur={() => handleRecordKeyBlur()}
              onkeydown={handleRecordKeyDown}
              onkeyup={handleRecordKeyUp}
            >
              {#if recordingTarget === "keys"}
                <div class="flex items-center gap-[10px]">
                  <span class="w-2 h-2 bg-accent-blue rounded-full animate-flash"></span>
                  <span class="text-[13px] font-semibold text-accent-blue">
                    {currentlyPressedKeys.length > 0
                      ? currentlyPressedKeys.join(" + ").replace(/KEY_/g, "")
                      : "Press your physical shortcut combination now..."}
                  </span>
                </div>
              {:else}
                <span class="text-[12px] text-obsidian-300 flex flex-col gap-1.5 items-center">
                  {#if editingBinding.keys.length > 0}
                    <div class="flex gap-1.5">
                      {#each editingBinding.keys as k}
                        <kbd class="px-1.5! py-0.5! text-[12px]! bg-accent-blue! text-black! border-none! font-extrabold! rounded!">{k.replace("KEY_", "")}</kbd>
                      {/each}
                    </div>
                    <span class="text-[10px] text-accent-blue opacity-80">(Click / Tab here to record)</span>
                  {:else}
                    ⚠️ Click/Focus here to press keys!
                  {/if}
                </span>
              {/if}
            </div>
          </div>

          {#if editingBinding.gesture === "chord"}
            <div class="flex flex-col gap-1.5">
              <h5 class="text-[11px] font-bold uppercase text-accent-blue tracking-[0.06em]">Subkey Trigger (Pressed Key)</h5>
              <div
                class={[
                  "border-2 rounded-desktop p-4 text-center cursor-pointer outline-none transition-all duration-200 flex flex-col items-center justify-center min-h-[70px]",
                  recordingTarget === "subkey"
                    ? "border-solid border-[#f43f5e] bg-[rgba(244,63,94,0.05)] animate-border-pulse"
                    : (editingBindingConflict
                      ? "border-solid border-[#fbbf24] bg-[rgba(251,191,36,0.05)] hover:border-[#fbbf24]/80"
                      : "border-dashed border-white/5 bg-black/25 hover:border-accent-blue hover:bg-black/35 focus:border-accent-blue focus:bg-black/35")
                ].join(" ")}
                tabindex="0"
                role="button"
                aria-label="Subkey recorder input"
                onclick={() => recordingTarget = "subkey"}
                onfocus={() => recordingTarget = "subkey"}
                onblur={() => handleRecordKeyBlur()}
                onkeydown={handleRecordKeyDown}
                onkeyup={handleRecordKeyUp}
              >
                {#if recordingTarget === "subkey"}
                  <div class="flex items-center gap-[10px]">
                    <span class="w-2 h-2 bg-accent-blue rounded-full animate-flash"></span>
                    <span class="text-[13px] font-semibold text-accent-blue">
                      Press the trigger key now...
                    </span>
                  </div>
                {:else}
                  <span class="text-[12px] text-obsidian-300 flex flex-col gap-1.5 items-center">
                    {#if editingBinding.subkey}
                      <kbd class="px-1.5! py-0.5! text-[12px]! bg-accent-blue! text-black! border-none! font-extrabold! rounded!">{editingBinding.subkey.replace("KEY_", "")}</kbd>
                      <span class="text-[10px] text-accent-blue opacity-80">(Click / Tab here to change trigger key)</span>
                    {:else}
                      ⚠️ Click/Focus here to press the trigger key!
                    {/if}
                  </span>
                {/if}
              </div>
            </div>
          {/if}

          {#if editingBindingConflict}
            <span class="validation-error-msg" style="color: #fbbf24; font-weight: 500; margin-top: 4px;">
              ⚠️ Warning: This key combination and gesture type conflicts with another existing binding.
            </span>
          {/if}
        </div>

        <!-- Ollama Post-Processing Settings -->
        <div class="processing-toggles border-t border-white/5 pt-[14px] mt-4">
          <h5>Ollama LLM Post-Processing</h5>
          <label class="checkbox-field">
            <input type="checkbox" bind:checked={editOllamaEnabled} />
            <span>Enable Ollama LLM post-processing for this hotkey</span>
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
        </div>
      </div>
      <div class="modal-footer">
        <button class="btn-action secondary" onclick={() => { editingBinding = null; recordingTarget = null; }}>Cancel</button>
        <button class="btn-action primary" onclick={saveBindingModal}>Done</button>
      </div>
    </div>
  </div>
{/if}

<style>
  @reference "tailwindcss";

  .routing-section {
    @apply flex flex-col gap-5 pb-10;
  }

  /* Sub-navigation tabs styling */
  .sub-nav {
    @apply flex gap-2 border-b border-[var(--border)] pb-2;
  }

  .sub-btn {
    @apply bg-transparent border-none p-2 px-4 text-xs font-semibold text-[var(--text-muted)] rounded-[var(--radius)] transition-all duration-150 ease-in-out;
  }

  .sub-btn:hover {
    @apply text-[var(--text)] bg-white/[0.04];
  }

  .sub-btn.active {
    @apply text-[var(--accent2)] bg-[var(--accent2)]/8;
  }

  /* Header & Title bar */
  .section-header {
    @apply flex justify-between items-center gap-5 mb-2;
  }

  .description {
    @apply text-xs text-[var(--text-muted)] mt-1;
  }

  /* Visual grid layout */
  .cards-grid {
    @apply grid grid-cols-[repeat(auto-fill,minmax(280px,1fr))] gap-4;
  }

  /* Card and Glassmorphism design */
  .card {
    @apply flex flex-col border border-[var(--border)] rounded-[var(--radius)] p-4 gap-3 transition-all duration-200 ease-in-out bg-[#1a1f2e]/40;
  }

  .card:hover {
    @apply border-[var(--accent2)] shadow-[0_4px_20px_rgba(0,0,0,0.3)] -translate-y-0.5;
  }

  .card.disabled {
    @apply opacity-55 border-white/[0.05];
  }

  .card-header {
    @apply flex justify-between items-start gap-2;
  }

  .card-header h4 {
    @apply m-0 text-sm font-semibold text-[var(--text)];
  }

  .badge {
    @apply text-[10px] py-0.5 px-2 rounded-xl font-semibold uppercase;
  }

  .badge.delivery {
    @apply bg-purple-500/15 text-purple-300 border border-purple-500/30;
  }

  .badge.gesture {
    @apply bg-[var(--accent2)]/15 text-[var(--accent2)] border border-[var(--accent2)]/30;
  }

  .badge.ollama {
    @apply bg-emerald-500/15 text-emerald-200 border border-emerald-500/30;
  }

  .card-body {
    @apply flex-1 flex flex-col gap-1.5 text-xs;
  }

  .info-row {
    @apply flex justify-between gap-3;
  }

  .info-label {
    @apply text-[var(--text-muted)];
  }

  .info-val {
    @apply text-[var(--text)] text-right;
  }

  .line-clamp {
    @apply line-clamp-1 break-all;
  }

  .keys-display {
    @apply flex flex-wrap gap-1;
  }

  .bindings-list {
    @apply flex flex-col gap-2;
  }

  .binding-item {
    @apply flex border border-[var(--border)] rounded-[var(--radius)] p-2.5 px-3.5 transition-all duration-200 ease-in-out bg-[#1a1f2e]/40;
  }

  .binding-item:hover {
    @apply border-[var(--accent2)] shadow-[0_4px_20px_rgba(0,0,0,0.3)];
  }

  .binding-item.disabled {
    @apply opacity-55 border-white/[0.05] bg-[repeating-linear-gradient(-45deg,transparent,transparent_10px,rgba(0,0,0,0.25)_10px,rgba(0,0,0,0.25)_20px)];
  }

  .binding-content {
    @apply flex-1 flex flex-col gap-1 min-w-0;
  }

  .binding-header-row {
    @apply flex items-center justify-between w-full;
  }

  .binding-title {
    @apply text-sm font-semibold text-[var(--text)];
  }

  .binding-row2 {
    @apply flex items-center justify-between gap-2;
  }

  .binding-targets {
    @apply text-xs text-[var(--text-muted)];
  }

  .binding-actions {
    @apply flex justify-end items-center gap-1.5 border-t border-white/[0.05] pt-2 mt-1;
  }

  .confirm-label {
    @apply text-[11px] text-[var(--text-muted)];
  }

  kbd {
    @apply bg-[var(--surface2)] border border-[var(--border)] rounded px-1.5 py-0.5 text-[11px] font-mono font-bold text-[var(--accent2)] shadow-[0_1px_0_rgba(0,0,0,0.4)];
  }

  .card-actions {
    @apply flex justify-end gap-1.5 border-t border-white/[0.05] pt-2.5 mt-1;
  }

  /* Empty state */
  .empty-state {
    @apply col-span-full flex flex-col items-center justify-center p-10 border-2 border-dashed border-[var(--border)] rounded-[var(--radius)] text-center bg-black/15;
  }

  .empty-icon {
    @apply text-[32px] mb-2;
  }

  .empty-state p {
    @apply text-xs text-[var(--text-muted)] m-0;
  }

  /* Button Overrides */
  .btn-action {
    @apply bg-[var(--surface2)] text-[var(--text)] border border-[var(--border)] rounded-[var(--radius)] p-1.5 px-3.5 text-xs font-semibold cursor-pointer transition-all duration-150 ease-out;
  }

  .btn-action:hover {
    @apply bg-[var(--border)] border-[var(--text-muted)];
  }

  .btn-action.primary {
    @apply bg-[var(--accent)] text-white border-none;
  }

  .btn-action.primary:hover {
    @apply opacity-90;
  }

  .btn-action.small {
    @apply p-1 px-2 text-[11px];
  }

  .btn-action.danger {
    @apply text-red-400 border-red-400/20;
  }

  .btn-action.danger:hover {
    @apply bg-red-400/10 border-red-400;
  }

  .btn-add-wide {
    @apply w-full bg-[var(--accent2)] text-white border-none rounded-[var(--radius)] py-1.5 text-xs font-bold cursor-pointer transition-all duration-150 ease-out mb-4 flex justify-center items-center gap-2 shadow-[0_2px_6px_rgba(56,189,248,0.15)];
  }

  .btn-add-wide:hover {
    @apply brightness-110 -translate-y-0.5 shadow-[0_4px_12px_rgba(56,189,248,0.3)];
  }
  
  .btn-add-wide:active {
    @apply translate-y-0;
  }

  /* Modal Dialog Glassmorphism style */
  .modal-backdrop {
    @apply fixed inset-0 bg-black/60 backdrop-blur-[4px] flex items-center justify-center z-[1000];
  }

  .modal {
    @apply w-[90%] max-w-[500px] max-h-[85vh] rounded-[var(--radius)] border border-[var(--border)] flex flex-col shadow-[0_10px_30px_rgba(0,0,0,0.5)] bg-[#1e2436];
  }

  .modal-header {
    @apply flex justify-between items-center p-4 px-5 border-b border-[var(--border)];
  }

  .modal-header h3 {
    @apply m-0 text-[15px] font-bold;
  }

  .close-btn {
    @apply bg-transparent border-none text-[var(--text-muted)] text-base cursor-pointer;
  }

  .close-btn:hover {
    @apply text-[var(--text)];
  }

  .modal-body {
    @apply p-5 overflow-y-auto flex flex-col gap-3.5;
  }

  .modal-footer {
    @apply flex justify-end gap-2.5 p-4 px-5 border-t border-[var(--border)];
  }

  /* Morph & Nested structures */
  .morph-section {
    @apply bg-white/[0.02] border border-dashed border-[var(--border)] rounded-[var(--radius)] p-3 flex flex-col gap-2.5;
  }

  .morph-section h5, .processing-toggles h5 {
    @apply m-0 mb-1 text-[11px] font-bold uppercase text-[var(--accent2)];
  }

  .processing-toggles {
    @apply border-t border-[var(--border)] pt-3.5 flex flex-col gap-2.5;
  }

  .checkbox-field {
    @apply flex items-center gap-2 cursor-pointer text-xs text-[var(--text-muted)];
  }

  .checkbox-field input[type="checkbox"] {
    @apply cursor-pointer;
  }

  .checkbox-field:hover {
    @apply text-[var(--text)];
  }

  .actions {
    @apply flex justify-end border-t border-[var(--border)] pt-3.5;
  }

  .btn-save {
    @apply bg-[var(--accent)] text-white border-none rounded-[var(--radius)] py-2 px-6 font-semibold text-xs cursor-pointer transition-opacity duration-150 ease-out;
  }

  .btn-save:hover {
    @apply opacity-90;
  }

  .btn-save:disabled {
    @apply opacity-40 cursor-not-allowed;
  }

  .font-semibold {
    @apply font-semibold;
  }

  .mt-2 {
    @apply mt-2;
  }

  /* Keyframe transitions for modals */
  .animate-fade-in {
    @apply animate-[fade-in_0.2s_cubic-bezier(0.16,1,0.3,1)];
  }

  @keyframes fade-in {
    from { opacity: 0; transform: scale(0.96); }
    to { opacity: 1; transform: scale(1); }
  }

  .ollama-target-settings {
    @apply border-l-2 border-[var(--accent)] pl-3.5 ml-2.5 mt-2.5 mb-3.5 flex flex-col gap-2;
  }
  
  .prompt-error-msg {
    @apply text-red-300 text-xs mt-1 font-medium;
  }
  
  textarea {
    @apply w-full bg-[var(--color-obsidian-950)] border border-[var(--border)] rounded-[var(--radius)] text-[var(--text)] p-2 px-3 text-[13px] font-mono resize-y outline-none shadow-[inset_0_2px_4px_rgba(0,0,0,0.2)] transition-all duration-150 ease-out;
  }
  
  textarea:hover {
    @apply border-white/15 bg-[var(--color-obsidian-900)];
  }
  
  textarea:focus {
    @apply border-[var(--accent2)] bg-[var(--color-obsidian-950)] shadow-[0_0_0_2px_rgba(56,189,248,0.15),_inset_0_2px_4px_rgba(0,0,0,0.2)];
  }
  
  .field.col {
    @apply flex-col items-start gap-1 w-full;
  }

  /* MCP & Ollama Redesigned Form Styles */
  .mcp-container {
    @apply flex flex-col gap-4 py-1.5;
  }

  .field-label-row {
    @apply flex justify-between items-center w-full mb-0.5;
  }

  .field-title {
    @apply text-[13px] font-semibold text-[var(--text)];
  }

  .field-tag {
    @apply text-[9px] p-0.5 px-1.5 bg-[var(--accent2)]/8 text-[var(--accent2)] border border-[var(--accent2)]/25 rounded uppercase font-bold tracking-wide leading-none;
  }

  .full-width-input {
    @apply w-full!;
  }

  p.hint {
    @apply m-0 text-[11px] text-[var(--text-muted)] leading-relaxed;
  }

  p.hint code {
    @apply bg-[var(--color-obsidian-950)] text-[var(--color-accent-blue)] p-0.5 px-1 rounded font-mono text-[10px] border border-[var(--border)];
  }

  textarea.has-error, input.has-error {
    @apply border-red-400! shadow-[0_0_0_2px_rgba(229,115,115,0.15),_inset_0_2px_4px_rgba(0,0,0,0.2)]!;
  }

  .validation-error-msg {
    @apply block mt-1 text-xs font-medium text-red-400 leading-normal;
  }

  .btn-add-inline {
    @apply bg-transparent border-none text-[var(--accent2)] text-[11px] font-bold cursor-pointer p-0.5 px-1.5 rounded transition-all duration-150 ease-out;
  }
  .btn-add-inline:hover {
    @apply bg-[var(--accent2)]/8 text-white;
  }

  .target-selects-list {
    @apply flex flex-col gap-2 w-full mt-1;
  }

  .target-select-row {
    @apply flex items-center gap-2 w-full;
  }

  .target-select-row select {
    @apply flex-1 w-full;
  }

  .btn-remove-inline {
    @apply flex items-center justify-center box-border;
    background-color: rgba(239, 68, 68, 0.08);
    border: 1px solid rgba(239, 68, 68, 0.2);
    color: #f87171;
    cursor: pointer;
    font-size: 12px;
    font-weight: bold;
    padding: 6px 12px;
    border-radius: var(--radius);
    transition: all 0.15s ease-out;
    height: 34px;
  }

  .btn-remove-inline:hover {
    @apply bg-red-500 border-red-500 text-white shadow-[0_2px_8px_rgba(239,68,68,0.25)];
  }

  .longer-display-label {
    @apply min-w-[255px]!;
  }

  /* Conflict detection styling */
  .binding-item.has-conflict {
    @apply border-amber-500/45!;
  }

  .binding-item.active-conflict {
    @apply bg-amber-500/6 relative;
  }

  .binding-item.active-conflict:hover {
    @apply border-amber-500/70! shadow-[0_4px_20px_rgba(251,191,36,0.15)];
  }

  .binding-title.has-conflict {
    @apply pr-[75px];
  }

  .conflict-marker {
    @apply absolute top-2.5 right-3.5 bg-amber-400 text-neutral-900 text-[9px] font-extrabold p-0.5 px-1.5 rounded shadow-[0_2px_6px_rgba(251,191,36,0.25)];
  }

  .conflict-alert-banner {
    @apply flex items-center gap-2.5 bg-amber-500/6 border border-amber-500/25 rounded-[var(--radius)] p-2.5 px-3.5 mb-3 text-xs text-amber-400 leading-normal;
  }

  .warning-icon {
    @apply text-base shrink-0;
  }
</style>
