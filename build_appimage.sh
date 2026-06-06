#!/usr/bin/env bash
# VoxCtr AppImage Compilation Script
#
# Automates the entire compilation, packaging, and bundling pipeline
# to produce a fully portable standalone AppImage in the workspace root.

set -euo pipefail

# ── Colors ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# ══════════════════════════════════════════════════════════════════════════════
# 1. Verification of appimagetool Wrapper
# ══════════════════════════════════════════════════════════════════════════════
step "Checking AppImage Compiler Toolchain"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

# Parse application metadata from tauri.conf.json
TAURI_CONF="src-tauri/tauri.conf.json"
if [ ! -f "$TAURI_CONF" ]; then
    fail "Could not find Tauri configuration file at $TAURI_CONF"
    exit 1
fi

if command -v jq &>/dev/null; then
    APP_NAME=$(jq -r '.productName' "$TAURI_CONF")
    APP_VERSION=$(jq -r '.version' "$TAURI_CONF")
else
    APP_NAME=$(grep -oP '"productName":\s*"\K[^"]+' "$TAURI_CONF" || echo "VoxCtr")
    APP_VERSION=$(grep -oP '"version":\s*"\K[^"]+' "$TAURI_CONF" || echo "0.1.0")
fi

# Ensure the raw binary is renamed
if [ -f "./appimagetool" ] && [ ! -f "./appimagetool.bin" ]; then
    info "Found raw appimagetool binary. Restructuring into wrapper setup..."
    mv appimagetool appimagetool.bin
    chmod +x appimagetool.bin
fi

# Create the wrapper if missing
if [ ! -f "./appimagetool" ]; then
    info "Creating headless FUSE-bypass wrapper script..."
    cat > ./appimagetool <<'EOF'
#!/usr/bin/env bash
export QT_QPA_PLATFORM=offscreen
exec "$(dirname "$0")/appimagetool.bin" --appimage-extract-and-run "$@"
EOF
    chmod +x ./appimagetool
fi

# Verify unsquashfs is installed (required for FUSE-less extraction of AppImage builders)
if ! command -v unsquashfs &>/dev/null; then
    fail "The 'unsquashfs' utility is not installed on your system!"
    info "Building or running AppImages in FUSE-less mode requires squashfs-tools."
    info "👉 Please run './install.sh' to install it automatically, or run:"
    info "   - Arch:   sudo pacman -S squashfs-tools"
    info "   - Ubuntu: sudo apt install squashfs-tools"
    info "   - Fedora: sudo dnf install squashfs-tools"
    echo ""
    exit 1
fi

ok "AppImage toolchain wrapper is verified and ready."

# ══════════════════════════════════════════════════════════════════════════════
# 2. Build Frontend (Vite / Svelte)
# ══════════════════════════════════════════════════════════════════════════════
step "Building Svelte Frontend Assets"

if [ ! -d "node_modules" ]; then
    info "Installing frontend node packages..."
    npm install
fi

info "Compiling frontend bundle..."
npm run build
ok "Frontend compiled successfully."

# ══════════════════════════════════════════════════════════════════════════════
# 3. Compile & Bundle Tauri / Rust App
# ══════════════════════════════════════════════════════════════════════════════
step "Compiling & Packaging Tauri Application"

# Inject our root folder into PATH so Tauri's bundler uses our wrapper
export PATH="$ROOT_DIR:$PATH"
export QT_QPA_PLATFORM=offscreen
export APPIMAGE_EXTRACT_AND_RUN=1

# Detect and inject common CUDA paths to PATH for CMake nvcc detection in non-interactive shells
for cuda_dir in "/opt/cuda/bin" "/usr/local/cuda/bin"; do
    if [ -d "$cuda_dir" ]; then
        export PATH="$cuda_dir:$PATH"
    fi
done

# Set CUDA home variables if found on the system
CUDA_FOUND=false
if [ -d "/opt/cuda" ]; then
    export CUDA_PATH="/opt/cuda"
    export CUDA_TOOLKIT_ROOT_DIR="/opt/cuda"
    export CUDAToolkit_ROOT="/opt/cuda"
    export CUDACXX="/opt/cuda/bin/nvcc"
    CUDA_FOUND=true
elif [ -d "/usr/local/cuda" ]; then
    export CUDA_PATH="/usr/local/cuda"
    export CUDA_TOOLKIT_ROOT_DIR="/usr/local/cuda"
    export CUDAToolkit_ROOT="/usr/local/cuda"
    export CUDACXX="/usr/local/cuda/bin/nvcc"
    CUDA_FOUND=true
fi

info "Running Tauri release compiler with headless PATH and CUDA injection..."
if [ "$CUDA_FOUND" = true ]; then
    info "CUDA detected. Compiling with GPU support..."
    npx tauri build -- --features cuda
else
    info "CUDA not detected. Compiling for CPU only..."
    npx tauri build
fi

ok "Compilation finished successfully."

# ══════════════════════════════════════════════════════════════════════════════
# 4. Relocate & Expose Portable AppImage
# ══════════════════════════════════════════════════════════════════════════════
step "Exposing Portable AppImage to Root"

# Locate compiled AppImage files in target bundle directories
BUNDLE_DIR="./target/release/bundle/appimage"
if [ ! -d "$BUNDLE_DIR" ]; then
    # Fallback to local cargo/tauri target configurations
    BUNDLE_DIR="./src-tauri/target/release/bundle/appimage"
fi

FOUND_APPIMAGES=( $(find "$BUNDLE_DIR" -maxdepth 1 -name "*.AppImage" 2>/dev/null || true) )

if [ ${#FOUND_APPIMAGES[@]} -eq 0 ]; then
    fail "Could not locate compiled AppImage bundle in target outputs!"
    exit 1
fi

LATEST_BUNDLE="${FOUND_APPIMAGES[0]}"
PORTABLE_PATH="./${APP_NAME}-${APP_VERSION}-x86_64.AppImage"
SYMLINK_PATH="./${APP_NAME}-latest-x86_64.AppImage"

info "Found compiled bundle: $LATEST_BUNDLE"
info "Moving and exposing portable versioned AppImage to root..."
cp "$LATEST_BUNDLE" "$PORTABLE_PATH"
chmod +x "$PORTABLE_PATH"

# Establish a latest symlink to maintain compatibility for local scripts/runners
ln -sf "$(basename "$PORTABLE_PATH")" "$SYMLINK_PATH"
info "Created latest symlink: $SYMLINK_PATH -> $PORTABLE_PATH"

echo ""
echo -e "${BOLD}==================================================${NC}"
echo -e "${BOLD}  Portable AppImage Compiled Successfully!${NC}"
echo -e "${BOLD}==================================================${NC}"
echo ""
echo "  Your fully standalone, portable application is ready:"
echo -e "    👉 ${GREEN}${PORTABLE_PATH}${NC} ($(du -sh "$PORTABLE_PATH" | cut -f1))"
echo -e "    👉 Symlink: ${GREEN}${SYMLINK_PATH}${NC}"
echo ""
echo "  To launch and test the application directly, run:"
echo "    $PORTABLE_PATH"
echo ""
