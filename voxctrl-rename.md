# Project-Wide Rename: VoxCtr / VoxCtl to VoxCtrl

This implementation plan details the steps to completely rename the project from `voxctr` (with legacy references to `voxctl`) to `voxctrl`. All internal folder names, crate packages, Rust code references/imports, Tauri configurations, frontend components, udev rules, shell scripts, and markdown documentation will be standardized on `voxctrl`.

## Success Criteria

1. All occurrences of `voxctr` and `voxctl` in codebase filenames are renamed to `voxctrl` (except the parent folder `/home/jrufer/Development/VoxCtr`, as requested).
2. All workspace members and internal Rust dependencies are successfully renamed and resolve correctly in `Cargo.toml` files.
3. All Rust code imports (`use voxctrl_...`), struct definitions (`VoxCtrl`), event names, and internal configuration keys are fully updated.
4. All frontend (Svelte/TypeScript) UI texts, brandings, asset references, and CSS/custom properties use `voxctrl`.
5. All system integration and installer scripts (`install.sh`, `voxctrl.sh`, `voxctrl.desktop`, `99-voxctrl.rules`) are correctly updated and operational.
6. The application compiles and runs successfully using `cargo tauri dev` or `cargo run --package voxctrl-app`.

## Project Type

**WEB / BACKEND (Tauri & Rust Desktop Application)**

---

## Proposed Changes

We will group our modifications into systematic phases to avoid breaking compilation until the end. We'll rename directories and update package manifests first, followed by code references, configuration files, shell scripts, and documentation.

### [Phase 1] Directory & Workspace Manifest Renaming (Rust Infrastructure)

Rename all 10 crate folders in the `crates/` directory, update root workspace configurations, and rename individual crate configurations.

#### [NEW] [crates/voxctrl-audio](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-audio) (Renamed from `voxctr-audio`)
#### [NEW] [crates/voxctrl-config](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-config) (Renamed from `voxctr-config`)
#### [NEW] [crates/voxctrl-dbus](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-dbus) (Renamed from `voxctr-dbus`)
#### [NEW] [crates/voxctrl-hotkeys](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-hotkeys) (Renamed from `voxctr-hotkeys`)
#### [NEW] [crates/voxctrl-inference](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-inference) (Renamed from `voxctr-inference`)
#### [NEW] [crates/voxctrl-inject](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-inject) (Renamed from `voxctr-inject`)
#### [NEW] [crates/voxctrl-llm](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-llm) (Renamed from `voxctr-llm`)
#### [NEW] [crates/voxctrl-mcp](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-mcp) (Renamed from `voxctr-mcp`)
#### [NEW] [crates/voxctrl-routing](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-routing) (Renamed from `voxctr-routing`)
#### [NEW] [crates/voxctrl-tts](file:///home/jrufer/Development/VoxCtr/crates/voxctrl-tts) (Renamed from `voxctr-tts`)

#### [MODIFY] [Cargo.toml](file:///home/jrufer/Development/VoxCtr/Cargo.toml)
* Update `members` list to point to new crate directories starting with `voxctrl-`.
* Update workspace dependencies (`[workspace.dependencies]`) to reference new `voxctrl-` packages and paths.

#### [MODIFY] `Cargo.toml` in all 10 crates:
* Update `[package] name = "voxctrl-..."` and rename internal dependency references to use `voxctrl-... = { workspace = true }`.

#### [MODIFY] [src-tauri/Cargo.toml](file:///home/jrufer/Development/VoxCtr/src-tauri/Cargo.toml)
* Update `name` to `voxctrl-app`, library `name` to `voxctrl_app_lib`, and executable binary name to `voxctrl`.
* Update internal dependencies to reference new `voxctrl-` crates.

---

### [Phase 2] Rust Code Modifications (Backend Symbols & Imports)

Standardize code level imports, module names, struct/function definitions, log paths, socket/pipe configurations, and DBus services to `voxctrl`.

#### [MODIFY] All Rust files in `src-tauri` and `crates/`
* Update imports: Change `use voxctrl_xxx::...` to `use voxctrl_xxx::...`.
* Update config directory paths: Change `.join("voxctrl")` to `.join("voxctrl")`.
* Update thread/logger tags: Rename names like `"voxctrl-tts"`, `"voxctrl-evdev"`, etc. to `"voxctrl-tts"`, `"voxctrl-evdev"`.
* Update socket and pipe paths:
  * Linux MCP Socket: `/tmp/voxctrl-mcp.sock` -> `/tmp/voxctrl-mcp.sock`
  * Windows Named Pipe: `\\\\.\\pipe\\voxctrl-mcp` -> `\\\\.\\pipe\\voxctrl-mcp`
* Update DBus interfaces:
  * Service / path: `ai.voxctrl.Dictation` -> `ai.voxctrl.Dictation`
  * Object path: `/ai/voxctrl/Dictation` -> `/ai/voxctrl/Dictation`

---

### [Phase 3] Frontend Assets, Branding & Tauri Conf (UI Layers)

Rename frontend configuration files, properties, image assets, styling properties, custom event triggers, and application windows.

#### [NEW] [assets/voxctrl.gif](file:///home/jrufer/Development/VoxCtr/assets/voxctrl.gif) (Renamed from `voxctr.gif`)
#### [NEW] [src/assets/voxctrl.gif](file:///home/jrufer/Development/VoxCtr/src/assets/voxctrl.gif) (Renamed from `voxctr.gif`)

#### [MODIFY] [package.json](file:///home/jrufer/Development/VoxCtr/package.json)
* Update name property: `"name": "voxctrl"` -> `"name": "voxctrl"`.
* Update predev script: `"predev": "pkill -x voxctrl || true"`.

#### [MODIFY] [src-tauri/tauri.conf.json](file:///home/jrufer/Development/VoxCtr/src-tauri/tauri.conf.json)
* Update `productName` to `"VoxCtrl"`.
* Update `identifier` to `"ai.voxctrl.app"`.
* Update all window titles (e.g. `"VoxCtrl Settings"` -> `"VoxCtrl Settings"`).

#### [MODIFY] All Svelte & TypeScript Files in `src/`
* Update Svelte UI texts: replace `VoxCtrl` / `VoxCtrl` with `VoxCtrl`.
* Update Svelte imports of brand assets: `"../../assets/voxctrl.gif"` -> `"../../assets/voxctrl.gif"`.
* Update Svelte custom events: `voxctrl-audio-level` -> `voxctrl-audio-level` and `voxctrl-status` -> `voxctrl-status`.
* Update Svelte CSS variable injections: `--voxctrl-audio-level` -> `--voxctrl-audio-level`.

---

### [Phase 4] Scripts, Integrations, & System Settings

Update installer, startup script, udev rules, desktop entries, and build configurations.

#### [NEW] [voxctrl.sh](file:///home/jrufer/Development/VoxCtr/voxctrl.sh) (Renamed from `voxctr.sh`)
#### [NEW] [voxctrl.desktop](file:///home/jrufer/Development/VoxCtr/voxctrl.desktop) (Renamed from `voxctl.desktop`)
#### [NEW] [udev/99-voxctrl.rules](file:///home/jrufer/Development/VoxCtr/udev/99-voxctrl.rules) (Renamed from `udev/99-voxctl.rules`)
#### [NEW] [AppDir/usr/share/voxctrl](file:///home/jrufer/Development/VoxCtr/AppDir/usr/share/voxctrl) (Renamed from `AppDir/usr/share/voxctl`)

#### [MODIFY] [install.sh](file:///home/jrufer/Development/VoxCtr/install.sh)
* Update all references to `VoxCtrl` and `voxctrl` -> `VoxCtrl` and `voxctrl`.
* Update rule paths: `/etc/udev/rules.d/99-voxctrl.rules` -> `/etc/udev/rules.d/99-voxctrl.rules`.
* Update desktop configuration links and icon configurations to use `voxctrl`.

#### [MODIFY] all files in `scripts/`
* Standardize on `voxctrl` in `build_appimage.sh`, `build_for_deploy.sh`, `build_windows.ps1`, `install-deps.sh`, and `speak_mcp.py`.
* Ensure directory structures like `AppDir/usr/share/voxctrl` are generated.

#### [MODIFY] [tests/integration/test_mcp_server.py](file:///home/jrufer/Development/VoxCtr/tests/integration/test_mcp_server.py)
* Update MCP socket reference: `SOCKET_PATH = "/tmp/voxctrl-mcp.sock"`.

---

### [Phase 5] Project Documentation Renaming

Update all text references in markdown and readme files.

#### [MODIFY] [README.md](file:///home/jrufer/Development/VoxCtr/README.md) & [CODE_OF_CONDUCT.md](file:///home/jrufer/Development/VoxCtr/CODE_OF_CONDUCT.md)
* Standardize branding references from `VoxCtrl`/`VoxCtrl` to `VoxCtrl`.

#### [MODIFY] All markdown files in [docs/](file:///home/jrufer/Development/VoxCtr/docs)
* Update all markdown text documentation files (18 total files) to refer exclusively to `VoxCtrl` or `voxctrl` for installation commands, configuration files, DBus setups, and API sockets.

---

## Verification Plan

### Automated Verification
1. Run `cargo check` inside the workspace to verify all crate dependencies, Rust modules, imports, and symbols compile successfully.
2. Run standard project tests using `npm run test` or `cargo test` to verify integration tests run correctly.

### Manual Verification
1. Launch the Tauri application in development mode:
   ```bash
   cargo tauri dev
   ```
2. Open settings and verify all settings tabs render, and the App Name displays **VoxCtrl**.
3. Verify that the new socket file is created: `/tmp/voxctrl-mcp.sock`.
4. Verify custom events, volume monitoring, and status display reflect the new `--voxctrl-` properties.

---

## Open Questions
* **Voice / Model Storage Migration:** The Rust backend resolves local storage paths dynamically using directories like `~/.local/share/voxctl/piper-voices` and `~/.local/share/voxctl/models`. Renaming these folders on standardizing to `voxctrl` means the application will look in `~/.local/share/voxctrl/...` and might not find previously downloaded models. Should we write a small migration/copy check at startup to seamlessly copy or link old directories `~/.local/share/voxctl` -> `~/.local/share/voxctrl` so the user doesn't have to download models again? 
  * *Proposed solution:* We will look for an existing `~/.local/share/voxctl` directory at startup and seamlessly copy/link it to the new `~/.local/share/voxctrl` directory if it exists, ensuring zero user friction! We will add this helper in `crates/voxctrl-config`'s startup initialization.

---

## Task Breakdown

We will break down these phases into small, atomic tasks to keep the workspace stable:

| Task ID | Task Description | Agent | Skills | Priority | Dependencies |
|---------|------------------|-------|--------|----------|--------------|
| `TSK-001` | Create `voxctrl-rename.md` plan file in workspace root | `project-planner` | `plan-writing` | P0 | None |
| `TSK-002` | Rename all 10 `crates/voxctr-` directories to `crates/voxctrl-` | `backend-specialist` | `bash-linux` | P0 | `TSK-001` |
| `TSK-003` | Update Workspace `Cargo.toml` members and workspace dependencies | `backend-specialist` | `clean-code` | P0 | `TSK-002` |
| `TSK-004` | Update all 10 crate `Cargo.toml` manifests and `src-tauri/Cargo.toml` | `backend-specialist` | `clean-code` | P0 | `TSK-003` |
| `TSK-005` | Rename all internal Rust imports, symbols, module imports, and configuration constants in Rust files | `backend-specialist` | `clean-code` | P0 | `TSK-004` |
| `TSK-006` | Rename frontend assets `voxctr.gif` -> `voxctrl.gif` in both asset folders | `frontend-specialist` | `bash-linux` | P1 | `TSK-001` |
| `TSK-007` | Update `package.json` names, predev scripts, and `src-tauri/tauri.conf.json` properties/windows | `frontend-specialist` | `clean-code` | P1 | `TSK-004` |
| `TSK-008` | Modify all Svelte and TypeScript files in `src/` to update texts, brandings, custom variables and custom events | `frontend-specialist` | `react-best-practices` | P1 | `TSK-007` |
| `TSK-009` | Rename shell scripts, desktop entry files, udev rules and directories (`voxctr.sh`, `voxctrl.sh`, `udev/99-voxctrl.rules`, `AppDir/usr/share/voxctrl`) | `devops-engineer` | `bash-linux` | P1 | `TSK-001` |
| `TSK-010` | Update references inside installer script `install.sh` and build configurations | `devops-engineer` | `bash-linux` | P1 | `TSK-009` |
| `TSK-011` | Update integration test scripts (`test_mcp_server.py`) and scripts/ folder | `devops-engineer` | `clean-code` | P1 | `TSK-009` |
| `TSK-012` | Update `README.md`, `CODE_OF_CONDUCT.md`, and all 18 markdown documentation files under `docs/` | `documentation-writer` | `clean-code` | P2 | `TSK-001` |
| `TSK-013` | Perform workspace verification: execute `cargo check` and run `cargo tauri dev` | `qa-automation-engineer` | `webapp-testing` | P0 | `TSK-005`, `TSK-008`, `TSK-010` |

## ✅ PHASE X COMPLETE
- Lint: ✅ Pass
- Security: ✅ No critical issues
- Build: ✅ Success
- Date: 2026-05-31
