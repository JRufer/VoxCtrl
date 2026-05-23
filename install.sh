#!/usr/bin/env bash
# VoxCtr Installer Script
#
# Detects system package manager, installs all system dependencies,
# and builds the Tauri/Rust application in release mode.

set -euo pipefail

# ── Colours ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'
BLUE='\033[0;34m'; BOLD='\033[1m'; NC='\033[0m'

ok()   { echo -e "  ${GREEN}[OK]${NC}   $*"; }
warn() { echo -e "  ${YELLOW}[WARN]${NC}  $*"; }
info() { echo -e "  ${BLUE}[*]${NC}    $*"; }
fail() { echo -e "  ${RED}[FAIL]${NC}  $*"; }
step() { echo -e "\n${BOLD}── $* ──────────────────────────────────────────${NC}"; }

# ── Package Manager Detection ────────────────────────────────────────────────
detect_pkg_manager() {
    if command -v pacman &>/dev/null;    then echo "pacman"
    elif command -v apt-get &>/dev/null; then echo "apt"
    elif command -v dnf &>/dev/null;     then echo "dnf"
    elif command -v zypper &>/dev/null;  then echo "zypper"
    else                                      echo "unknown"
    fi
}

PKG_MGR=$(detect_pkg_manager)

# ══════════════════════════════════════════════════════════════════════════════
# 1. Install System Dependencies
# ══════════════════════════════════════════════════════════════════════════════
step "Installing System Dependencies"

info "Detected Package Manager: $PKG_MGR"

case "$PKG_MGR" in
    pacman)
        info "Running pacman system update and installing dependencies..."
        sudo pacman -S --noconfirm --needed \
            base-devel rustup nodejs npm pkgconf \
            webkit2gtk-4.1 openssl libayatana-appindicator3 \
            wtype xdotool wl-clipboard xclip portaudio
        ;;
    apt)
        info "Running apt-get update and installing dependencies..."
        sudo apt-get update -y
        sudo apt-get install -y \
            build-essential curl nodejs npm pkg-config \
            libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev \
            wtype xdotool wl-clipboard xclip portaudio19-dev
        ;;
    dnf)
        info "Running dnf package installation..."
        sudo dnf groupinstall -y "Development Tools"
        sudo dnf install -y \
            curl nodejs npm pkgconf-pkg-config \
            webkit2gtk4.1-devel openssl-devel libayatana-appindicator3-devel \
            wtype xdotool wl-clipboard xclip portaudio-devel
        ;;
    zypper)
        info "Running zypper package installation..."
        sudo zypper install -t pattern -y devel_basis
        sudo zypper install -y \
            curl nodejs npm pkg-config \
            webkit2gtk3-devel libopenssl-devel libayatana-appindicator3-devel \
            wtype xdotool wl-clipboard xclip portaudio-devel
        ;;
    *)
        fail "Unsupported package manager. Please install dependencies manually:"
        echo "  - Rust compiler & Cargo"
        echo "  - Node.js & npm (v18+)"
        echo "  - webkit2gtk-4.1 dev libraries"
        echo "  - openssl / libssl dev libraries"
        echo "  - libayatana-appindicator3 dev libraries"
        echo "  - portaudio dev libraries"
        echo "  - xdotool, wtype, xclip, wl-clipboard"
        exit 1
        ;;
esac

ok "System dependencies installed successfully."

# ══════════════════════════════════════════════════════════════════════════════
# 2. Rust Compiler Toolchain setup
# ══════════════════════════════════════════════════════════════════════════════
step "Setting up Rust Compiler"

if ! command -v cargo &>/dev/null; then
    info "Rust toolchain is not fully initialized. Installing via rustup..."
    if command -v rustup &>/dev/null; then
        rustup default stable
    else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi
fi
ok "Rust toolchain ready: $(cargo --version)"

# ══════════════════════════════════════════════════════════════════════════════
# 3. Build Node Frontend Dependencies
# ══════════════════════════════════════════════════════════════════════════════
step "Building Frontend (npm)"

info "Installing frontend node packages..."
npm install

info "Building frontend assets..."
npm run build
ok "Frontend assets built successfully."

# ══════════════════════════════════════════════════════════════════════════════
# 4. Compile Tauri App
# ══════════════════════════════════════════════════════════════════════════════
step "Compiling Tauri / Rust Application"

info "Installing Tauri CLI locally..."
npm install @tauri-apps/cli --save-dev

info "Compiling application in release mode..."
npx tauri build

if [ -f "./target/release/voxctr" ]; then
    ok "Application compiled successfully! Binary: ./target/release/voxctr"
else
    fail "Compilation finished but binary was not found where expected."
    exit 1
fi

# ══════════════════════════════════════════════════════════════════════════════
# 5. Udev Rules Setup for evdev Hotkeys
# ══════════════════════════════════════════════════════════════════════════════
step "Configuring Hardware permissions (udev)"

UDEV_RULE_PATH="/etc/udev/rules.d/99-voxctl.rules"
if [ ! -f "$UDEV_RULE_PATH" ]; then
    info "Setting up udev rules for global hotkeys (requires sudo)..."
    sudo tee "$UDEV_RULE_PATH" > /dev/null <<EOF
KERNEL=="uinput", GROUP="input", MODE="0660", OPTIONS+="static_node=uinput"
EOF
    info "Reloading udev rules..."
    sudo udevadm control --reload-rules && sudo udevadm trigger || true
    ok "udev rules configured successfully."
else
    ok "udev rules already exist."
fi

echo ""
echo -e "${BOLD}==========================================${NC}"
echo -e "${BOLD}  Installation Complete!${NC}"
echo -e "${BOLD}==========================================${NC}"
echo ""
echo "  To start the application, run:"
echo "    ./voxctr.sh"
echo ""
echo "  Note: If you just set up hotkeys for the first time, you may"
echo "  need to ensure your user is in the 'input' group and log out/in."
echo ""
