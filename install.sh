#!/bin/bash
# Whisper-Wayland AppImage Installer

set -e

APP_NAME="Whisper-Wayland"
APPIMAGE_FILE="Whisper-Wayland-x86_64.AppImage"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"
ICON_DIR="$HOME/.local/share/icons/hicolor/256x256/apps"

echo "=========================================="
echo " Installing $APP_NAME "
echo "=========================================="

# Check if AppImage exists
if [ ! -f "$APPIMAGE_FILE" ]; then
    echo "Error: $APPIMAGE_FILE not found in the current directory."
    echo "Please build the AppImage first or place this script next to it."
    exit 1
fi

# Ensure target directories exist
mkdir -p "$BIN_DIR"
mkdir -p "$DESKTOP_DIR"
mkdir -p "$ICON_DIR"

# 1. Install the AppImage
echo "[*] Copying AppImage to $BIN_DIR..."
cp "$APPIMAGE_FILE" "$BIN_DIR/whisper-wayland"
chmod +x "$BIN_DIR/whisper-wayland"

# 2. Install the icon
echo "[*] Installing icon..."
if [ -f "assets/app_icon.png" ]; then
    cp assets/app_icon.png "$ICON_DIR/whisper-wayland.png"
fi

# 3. Create the desktop entry
echo "[*] Creating desktop entry..."
cat > "$DESKTOP_DIR/whisper-wayland.desktop" <<EOF
[Desktop Entry]
Name=Whisper Wayland
Comment=Native, on-device voice-to-text pipeline
Exec=$BIN_DIR/whisper-wayland
Icon=whisper-wayland
Terminal=false
Type=Application
Categories=Utility;Audio;
EOF

# 4. Set up permissions (evdev)
echo ""
echo "[*] Setting up hardware permissions (evdev) for global hotkeys..."
echo "This step requires root privileges to create udev rules."
if [ -f "scripts/setup-permissions.sh" ]; then
    sudo bash scripts/setup-permissions.sh
else
    echo "Warning: scripts/setup-permissions.sh not found. Hotkeys may not work globally."
fi

# 5. Update desktop database
echo "[*] Updating desktop database..."
if command -v update-desktop-database &> /dev/null; then
    update-desktop-database "$DESKTOP_DIR"
fi

echo "=========================================="
echo " Installation Complete!"
echo " "
echo " Note: Make sure '$BIN_DIR' is in your PATH."
echo " You may need to log out and log back in for the permission changes to take effect."
echo "=========================================="
