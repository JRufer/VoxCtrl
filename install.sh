#!/usr/bin/env bash
# VoxCtr Installer & Host Setup Script
#
# Configures the host environment to run the portable AppImage natively:
# 1. Installs system runtime dependencies (PortAudio, WebKitGTK, tools).
# 2. Ensures the portable AppImage exists (compiling if missing).
# 3. Establishes hardware udev permissions for evdev global hotkeys.
# 4. Integrates the AppImage into the desktop launcher (~/.local/share/applications/).

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
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

# ══════════════════════════════════════════════════════════════════════════════
# 1. Install Host Runtime Dependencies
# ══════════════════════════════════════════════════════════════════════════════
step "Installing Host Runtime Dependencies"

info "Detected Package Manager: $PKG_MGR"

case "$PKG_MGR" in
    pacman)
        info "Installing portable runtime packages via pacman..."
        sudo pacman -S --noconfirm --needed \
            webkit2gtk-4.1 openssl libayatana-appindicator \
            wtype xdotool wl-clipboard xclip portaudio squashfs-tools
        ;;
    apt)
        info "Installing portable runtime packages via apt..."
        sudo apt-get update -y
        sudo apt-get install -y \
            libwebkit2gtk-4.1-dev libssl-dev libayatana-appindicator3-dev \
            wtype xdotool wl-clipboard xclip portaudio19-dev squashfs-tools
        ;;
    dnf)
        info "Installing portable runtime packages via dnf..."
        sudo dnf install -y \
            webkit2gtk4.1-devel openssl-devel libayatana-appindicator3-devel \
            wtype xdotool wl-clipboard xclip portaudio-devel squashfs-tools
        ;;
    zypper)
        info "Installing portable runtime packages via zypper..."
        sudo zypper install -y \
            webkit2gtk3-devel libopenssl-devel libayatana-appindicator3-devel \
            wtype xdotool wl-clipboard xclip portaudio-devel squashfs-tools
        ;;
    *)
        warn "Unsupported package manager. Please ensure you have these runtimes installed manually:"
        echo "  - webkit2gtk-4.1 libraries"
        echo "  - openssl (libssl)"
        echo "  - libayatana-appindicator3"
        echo "  - portaudio libraries"
        echo "  - wtype (Wayland keystrokes) / xdotool (X11 keystrokes)"
        echo "  - wl-clipboard (Wayland clipboard) / xclip (X11 clipboard)"
        ;;
esac

ok "Host runtime dependencies installed."

# ══════════════════════════════════════════════════════════════════════════════
# 2. Retrieve / Compile AppImage
# ══════════════════════════════════════════════════════════════════════════════
step "Retrieving Portable AppImage"

PORTABLE_APPIMAGE="./VoxCtl-x86_64.AppImage"

if [ -f "$PORTABLE_APPIMAGE" ]; then
    ok "Portable AppImage found in workspace root: $PORTABLE_APPIMAGE"
else
    info "Portable AppImage not found in root. Attempting to fetch pre-compiled binary..."
    
    DOWNLOAD_URL="https://github.com/JRufer/VoxCtr/releases/latest/download/VoxCtl-x86_64.AppImage"
    FETCHED=0
    
    if command -v curl &>/dev/null; then
        info "Downloading latest AppImage via curl..."
        if curl -s -L -f -o "$PORTABLE_APPIMAGE" "$DOWNLOAD_URL"; then
            FETCHED=1
        fi
    elif command -v wget &>/dev/null; then
        info "Downloading latest AppImage via wget..."
        if wget -q -O "$PORTABLE_APPIMAGE" "$DOWNLOAD_URL"; then
            FETCHED=1
        fi
    fi
    
    if [ $FETCHED -eq 1 ]; then
        chmod +x "$PORTABLE_APPIMAGE"
        ok "Fetched latest pre-compiled AppImage successfully!"
    else
        warn "Could not fetch pre-compiled binary (it may not be released yet or network is offline)."
        info "Falling back to compiling AppImage from source..."
        
        # Check for build toolchain dependencies
        MISSING_BUILD_TOOLS=0
        if ! command -v cargo &>/dev/null; then MISSING_BUILD_TOOLS=1; fi
        if ! command -v npm &>/dev/null;   then MISSING_BUILD_TOOLS=1; fi
        
        if [ $MISSING_BUILD_TOOLS -eq 1 ]; then
            info "Compiler tools are missing. Installing build toolchain dependencies..."
            case "$PKG_MGR" in
                pacman)
                    sudo pacman -S --noconfirm --needed base-devel rustup nodejs npm pkgconf
                    ;;
                apt)
                    sudo apt-get install -y build-essential curl nodejs npm pkg-config
                    ;;
                dnf)
                    sudo dnf groupinstall -y "Development Tools"
                    sudo dnf install -y curl nodejs npm pkgconf-pkg-config
                    ;;
                zypper)
                    sudo zypper install -t pattern -y devel_basis
                    sudo zypper install -y curl nodejs npm pkg-config
                    ;;
            esac
            
            # Initialize rustup if needed
            if ! command -v cargo &>/dev/null && command -v rustup &>/dev/null; then
                rustup default stable
                source "$HOME/.cargo/env" || true
            fi
        fi
        
        # Run the compiler script
        info "Executing build_appimage.sh to compile portable package..."
        ./build_appimage.sh
        ok "AppImage compiled successfully from source."
    fi
fi

# ══════════════════════════════════════════════════════════════════════════════
# 3. Udev Rules Setup for evdev Hotkeys
# ══════════════════════════════════════════════════════════════════════════════
step "Configuring Hardware Permissions (udev)"

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

# Add active user to input group if not already present
if ! groups "$USER" | grep -q "\binput\b"; then
    info "Adding user '$USER' to 'input' group for hardware keystroke capture..."
    sudo usermod -aG input "$USER"
    warn "You have been added to the 'input' group. You MUST log out and log back in for hotkey bindings to work!"
else
    ok "User is already in the 'input' group."
fi

# ══════════════════════════════════════════════════════════════════════════════
# 4. Desktop Integration launcher
# ══════════════════════════════════════════════════════════════════════════════
step "Registering Desktop Launcher & Application Icon"

ICON_DEST_DIR="$HOME/.local/share/icons/hicolor/128x128/apps"
LAUNCHER_DEST_DIR="$HOME/.local/share/applications"

mkdir -p "$ICON_DEST_DIR"
mkdir -p "$LAUNCHER_DEST_DIR"

# Install high-res desktop icon
if [ -f "./src-tauri/icons/128x128.png" ]; then
    cp "./src-tauri/icons/128x128.png" "$ICON_DEST_DIR/voxctl.png"
    ok "Application icon installed: $ICON_DEST_DIR/voxctl.png"
fi

# Write desktop entry linked directly to the portable AppImage
LAUNCHER_PATH="$LAUNCHER_DEST_DIR/voxctl.desktop"
ABS_APPIMAGE_PATH="$(readlink -f "$PORTABLE_APPIMAGE")"

cat > "$LAUNCHER_PATH" <<EOF
[Desktop Entry]
Name=VoxCtr
Comment=Private Global Voice Dictation Gateway
Exec=$ABS_APPIMAGE_PATH
Icon=voxctl
Terminal=false
Type=Application
Categories=Utility;AudioVideo;
StartupNotify=false
Keywords=whisper;voice;dictation;wayland;
EOF

chmod +x "$LAUNCHER_PATH"
ok "Desktop launcher integrated successfully: $LAUNCHER_PATH"

echo ""
echo -e "${BOLD}==================================================${NC}"
echo -e "${BOLD}  Setup & Integration Complete!${NC}"
echo -e "${BOLD}==================================================${NC}"
echo ""
echo "  VoxCtr is now fully integrated into your desktop environment!"
echo "  You can launch it directly from your applications menu or run:"
echo -e "    ${GREEN}$PORTABLE_APPIMAGE${NC}"
echo ""
echo "  ⚠️  IMPORTANT REMINDER:"
echo "  If you were just added to the 'input' group, you MUST log out"
echo "  and log back in (or reboot) for evdev hotkeys to function correctly."
echo ""
