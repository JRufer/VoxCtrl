# Development Guide

## Prerequisites

### Required Tools
- **Rust** 1.75+ with `cargo` — [rustup.rs](https://rustup.rs/)
- **Node.js** 18+ with `npm`
- **Tauri CLI** 2.x — `cargo install tauri-cli`

### Linux System Dependencies
```bash
# Ubuntu / Debian
sudo apt install \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libssl-dev \
  libasound2-dev \
  libspeechd-dev \
  pkg-config \
  build-essential

# Fedora
sudo dnf install \
  webkit2gtk4.1-devel \
  libayatana-appindicator-devel \
  openssl-devel \
  alsa-lib-devel \
  speech-dispatcher-devel
```

### Windows
- Visual Studio Build Tools 2019+
- WebView2 SDK (usually pre-installed)

---

## Repository Layout

```
VoxCtr/
├── src/                    # Svelte frontend
│   ├── main.ts
│   ├── App.svelte
│   ├── stores/
│   │   ├── config.ts
│   │   └── status.ts
│   └── lib/
│       ├── Settings/
│       ├── Overlay/
│       └── History/
│
├── src-tauri/              # Tauri application shell
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── src/
│       ├── main.rs
│       ├── lib.rs          # Main coordinator
│       ├── commands.rs     # IPC command handlers
│       └── state.rs        # AppState definition
│
├── crates/                 # Backend library crates
│   ├── voxctr-config/
│   ├── voxctr-audio/
│   ├── voxctr-hotkeys/
│   ├── voxctr-inference/
│   ├── voxctr-routing/
│   ├── voxctr-inject/
│   ├── voxctr-tts/
│   ├── voxctr-mcp/
│   ├── voxctr-dbus/
│   └── voxctr-llm/
│
├── Cargo.toml              # Workspace definition
├── package.json            # Frontend deps
├── vite.config.ts
└── svelte.config.js
```

---

## Development Workflow

### Start Dev Server
```bash
npm install          # Install frontend deps (first time only)
npm run tauri dev    # Start Tauri + Vite in development mode
```

This:
1. Starts Vite dev server on `http://localhost:5173` with HMR
2. Compiles the Rust backend
3. Launches the app with the WebView pointed at Vite

Svelte changes hot-reload instantly. Rust changes trigger a backend recompile (typically 5–30s).

### Frontend Only
If you only need to work on the UI:
```bash
npm run dev
# Opens http://localhost:5173 in browser
# Note: Tauri commands won't work in browser — mock them if needed
```

### Backend Only
```bash
cargo build -p voxctr-inference  # Build a specific crate
cargo test -p voxctr-config      # Test a specific crate
cargo check --workspace          # Type-check all crates
```

---

## Building for Production

### AppImage (Linux)
```bash
bash build_appimage.sh
# Output: VoxCtr.AppImage in project root
```

The build script:
1. Runs `npm run tauri build` to produce a `.deb` bundle
2. Extracts the contents into an AppDir
3. Runs `appimagetool` to create the AppImage

### Standard Tauri Build
```bash
npm run tauri build
# Output: src-tauri/target/release/bundle/
#   Linux:   .deb, .AppImage
#   Windows: .msi, .exe (NSIS)
```

---

## Crate Development Guide

Each crate under `crates/` is self-contained. They are included in the workspace `Cargo.toml` and referenced by `src-tauri` as path dependencies.

### Adding a new crate
```bash
cargo new --lib crates/voxctr-myfeature

# Add to Cargo.toml workspace members:
[workspace]
members = [
  ...
  "crates/voxctr-myfeature",
]

# Reference from src-tauri/Cargo.toml:
voxctr-myfeature = { path = "../crates/voxctr-myfeature" }
```

### Crate Conventions
- Keep each crate focused on one domain
- Expose a minimal public API (`pub` on types/functions needed by callers)
- Use `tokio` for async where I/O is needed; keep CPU-heavy work on dedicated OS threads
- Pass channels rather than `Arc<Mutex<_>>` for data pipelines where possible

---

## Key Files to Understand

### `src-tauri/src/lib.rs`
The main coordinator. This is where the audio pipeline is assembled:
- Creates all channels
- Spawns the hotkey listener
- Spawns the audio recorder
- Spawns the inference worker
- Starts the MCP server
- Starts the DBus service
- Runs the Tauri event loop with the status ticker

When adding a new integration, this is typically where you wire it in.

### `src-tauri/src/commands.rs`
All `#[tauri::command]` handlers. Each command is a thin wrapper that reads/writes `AppState` or calls into a crate. Keep commands small — business logic belongs in crates.

### `crates/voxctr-config/src/lib.rs`
The `AppConfig` struct is the source of truth for all settings. If you add a config option, add it here first, then expose it in the Settings UI.

### `crates/voxctr-routing/src/models.rs`
Defines `OutputTarget`, `HotkeyBinding`, `DeliveryType`, `TargetProcessingConfig`, and `GestureType`. Add new delivery types or target fields here.

---

## Adding a New Output Target Type

1. Add a variant to `DeliveryType` enum in `crates/voxctr-routing/src/models.rs`
2. Add any target-specific fields to `OutputTarget` in `crates/voxctr-routing/src/models.rs`
3. Add a match arm in the router dispatch logic in `crates/voxctr-routing/src/router.rs`
4. Update the TypeScript `OutputTarget` interface in `src/stores/config.ts`
5. Add the new type to the delivery type selector in `src/lib/Settings/RoutingTab.svelte`
6. Document in `docs/routing.md`

---

## Testing

```bash
# Run all tests
cargo test --workspace

# Run tests for a specific crate
cargo test -p voxctr-config
cargo test -p voxctr-routing

# Frontend type checking
npm run check    # svelte-check + tsc
```

There are currently no end-to-end tests. The audio pipeline is tested manually via the dev server.

### Simulating and Testing udev Diagnostics

Linux global hotkeys require specific udev permissions and user group memberships configured by `install.sh`. To make testing these startup states safe and easy, developers can use the `VOXCTR_TEST_UDEV_STATUS` environment variable to mock various diagnostic outcomes without mutating their own user accounts or system rules.

#### Why Test This?
* **Onboarding Verification**: Ensure that new users are clearly prompted to install required dependencies.
* **Troubleshooting Relogins**: Verify the specific advice prompting the user to reboot or log out if they ran `install.sh` but didn't refresh their session.
* **Layout Integrity**: Make sure the modal overlays perfectly on the dark obsidian theme on launch.

#### Mock Configurations

* **Simulate Missing Setup (Installer never run)**:
  Simulates `/etc/udev/rules.d/99-voxctr.rules` does not exist:
  ```bash
  VOXCTR_TEST_UDEV_STATUS=missing npm run tauri dev
  ```
  * **UI Outcome**: Spawns a standalone native window (`udev-warning`) in the foreground detailing the need for hardware udev rules, providing a direct **📥 Download install.sh** button to GitHub, and a **Continue Anyway** native window close pathway.

* **Simulate Relogin Required (Installer run but session not updated)**:
  Simulates that rules exist but the current shell process is missing `input` group permissions:
  ```bash
  VOXCTR_TEST_UDEV_STATUS=relogin npm run tauri dev
  ```
  * **UI Outcome**: Spawns a standalone native window (`udev-warning`) displaying the explicit logout/relogin guidance (hiding the installer download CTA since the rules are already present).

* **Simulate Normal/Configured State (Bypasses checks)**:
  ```bash
  VOXCTR_TEST_UDEV_STATUS=ok npm run tauri dev
  ```
  * **UI Outcome**: Spawns only the standard settings window; the diagnostic warning window remains completely hidden.


---

## Debugging

### Rust Logging
VoxCtr uses the `log` crate with `env_logger`. Enable verbose output:
```bash
RUST_LOG=debug npm run tauri dev
RUST_LOG=voxctr_inference=trace npm run tauri dev
```

### Frontend DevTools
In dev mode, right-click the Tauri window → Inspect Element to open WebKit DevTools.

### IPC Tracing
Add `console.log` around `invoke()` calls in Svelte, or add `println!` in command handlers in Rust.

### Audio Issues
```bash
# Check CPAL devices
RUST_LOG=cpal=debug npm run tauri dev

# Check PulseAudio
pactl list sources short
```
