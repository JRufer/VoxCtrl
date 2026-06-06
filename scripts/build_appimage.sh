#!/bin/bash
# Forward to the root AppImage build script to compile the Tauri application
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${SCRIPT_DIR}/../build_appimage.sh" "$@"
