#!/usr/bin/env bash
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYTHON="$SCRIPT_DIR/venv/bin/python"
LOG_DIR="$HOME/.local/share/whisper-wayland"
LOG="$LOG_DIR/app.log"
PID_FILE="$LOG_DIR/app.pid"

mkdir -p "$LOG_DIR"

# Prevent multiple instances
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Whisper-Wayland is already running (PID $(cat "$PID_FILE"))"
    exit 0
fi

nohup "$PYTHON" "$SCRIPT_DIR/src/main.py" > "$LOG" 2>&1 &
echo $! > "$PID_FILE"
echo "Whisper-Wayland started (PID $!, log: $LOG)"
