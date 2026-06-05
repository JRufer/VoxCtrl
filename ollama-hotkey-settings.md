# Move Ollama Post-Processing Settings to Hotkey Bindings Plan

This plan tracks the tasks to relocate the Ollama post-processing settings from target-level to hotkey-level, updating both the frontend UI and the backend coordinator/inference logic.

## Task Breakdown

### Phase 1: Rust Backend Models & Loader Updates
- [ ] Update `crates/voxctrl-routing/src/models.rs` to move Ollama fields from target configuration to hotkey configuration.
  - **Verify**: Code compiles.
- [ ] Update `crates/voxctrl-routing/src/loader.rs` to map the new fields in `RawBinding` / `HotkeyBinding` and remove them from `RawProcessing` / `OutputTarget`.
  - **Verify**: `cargo test -p voxctrl-routing` passes.

### Phase 2: Tauri App State & Audio Coordinator Updates
- [ ] Update `src-tauri/src/state.rs` to add `active_binding_id` to `AppState`.
  - **Verify**: Code compiles.
- [ ] Update `src-tauri/src/lib.rs` to record the triggered binding's ID when starting a gesture, and forward it in `InferenceRequest` to the inference engine.
  - **Verify**: Code compiles.
- [ ] Update `src-tauri/src/commands.rs` to clear the active binding ID when recording is started via UI/direct commands.
  - **Verify**: Code compiles.

### Phase 3: Inference Engine Processing updates
- [ ] Update `crates/voxctrl-inference/src/lib.rs` to accept `binding_id` in `InferenceRequest`, load bindings, and run Ollama post-processing on the active hotkey binding config rather than the target config.
  - **Verify**: `cargo test -p voxctrl-inference` compiles.

### Phase 4: Frontend UI Updates
- [ ] Update `src/lib/Settings/routing-types.ts` to adjust TypeScript interfaces.
  - **Verify**: Frontend compiles.
- [ ] Update `src/lib/Settings/RoutingTab.svelte` to remove Ollama settings from the target editor modal and place them in the hotkey binding editor modal, handling all saving, loading, validation, and defaults correctly.
  - **Verify**: Svelte dev builds successfully without TS issues.

### Phase 5: Verification & Audit
- [ ] Run all automated unit tests: `cargo test`.
- [ ] Run the verify/checklist scripts as required.
- [ ] Manually test hotkey recording with Ollama post-processing.
