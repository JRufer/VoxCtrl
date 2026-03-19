#!/bin/bash
# Whisper Wayland AppImage Build Script
# This script creates a portable AppImage bundling Python, CUDA libs, and dependencies.

# Exit on error
set -e

APP_NAME="Whisper-Wayland"
APP_DIR="AppDir"
PYTHON_VERSION="3.14" # Match host python version

echo "Building AppImage for ${APP_NAME}..."

# 1. Clean previous builds
rm -rf ${APP_DIR} *.AppImage
mkdir -p ${APP_DIR}/usr/bin
mkdir -p ${APP_DIR}/usr/lib/python${PYTHON_VERSION}/site-packages
mkdir -p ${APP_DIR}/usr/share/whisper-wayland
mkdir -p ${APP_DIR}/usr/share/metainfo
mkdir -p ${APP_DIR}/usr/share/applications
mkdir -p ${APP_DIR}/usr/share/icons/hicolor/256x256/apps

# 2. Copy application source
cp -r src/ ${APP_DIR}/usr/share/whisper-wayland/
cp -r assets/ ${APP_DIR}/usr/share/whisper-wayland/
cp whisper-wayland.desktop ${APP_DIR}/usr/share/applications/

# Update desktop file for AppImage (remove absolute paths)
sed -i "s|^Exec=.*|Exec=python3 /usr/share/whisper-wayland/src/main.py|" ${APP_DIR}/usr/share/applications/whisper-wayland.desktop
sed -i "s|^Icon=.*|Icon=whisper-wayland|" ${APP_DIR}/usr/share/applications/whisper-wayland.desktop

cp assets/app_icon.png ${APP_DIR}/usr/share/icons/hicolor/256x256/apps/whisper-wayland.png
cp assets/app_icon.png ${APP_DIR}/whisper-wayland.png
ln -sf usr/share/applications/whisper-wayland.desktop ${APP_DIR}/whisper-wayland.desktop

# 3. Install Dependencies into AppDir
echo "Installing dependencies into AppDir..."
pip install --target ${APP_DIR}/usr/lib/python${PYTHON_VERSION}/site-packages \
    faster-whisper \
    PyQt6 \
    pyaudio \
    evdev \
    nvidia-cublas-cu12 \
    nvidia-cudnn-cu12

# 4. Copy AppRun (entry point)
cp AppRun ${APP_DIR}/AppRun
chmod +x ${APP_DIR}/AppRun

# 5. Download AppImageTool if not present
if ! command -v appimagetool &> /dev/null; then
    echo "appimagetool not found, downloading..."
    wget -c https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage -O appimagetool
    chmod +x appimagetool
    TOOL="./appimagetool"
else
    TOOL="appimagetool"
fi

# 6. Build the AppImage
export ARCH=x86_64
$TOOL ${APP_DIR} ${APP_NAME}-x86_64.AppImage

echo "Success! ${APP_NAME}-x86_64.AppImage created."
