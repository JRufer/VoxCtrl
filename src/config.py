import json
import os
from pathlib import Path

DEFAULT_CONFIG = {
    "hotkey": ["KEY_LEFTMETA", "KEY_SPACE"],
    "toggle_hotkey": ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"],
    "inference_mode": "Balanced",
    "model_size": "base",
    "device": "auto",  # cuda or cpu
    "compute_type": "default",
    "vad_threshold": 0.5,
    "min_silence_duration_ms": 500,
    "input_device_index": None,  # For PyAudio
    "evdev_device": None,         # e.g. /dev/input/event0
    "show_overlay": True,
    "remove_fillers": True,
    "custom_vocabulary": [],
    # P0 features
    "spoken_punctuation": True,   # Replace "period", "comma", etc. with symbols
    "auto_format_lists": True,    # Reformat "1. X 2. Y" → newline-separated list
    "quiet_mode": False,          # Lower VAD threshold + raise gain for whispering
    "show_notification": False,   # notify-send popup after each transcription
}

CONFIG_PATH = Path.home() / ".config" / "whisper-wayland" / "config.json"

class Config:
    def __init__(self):
        self.config = DEFAULT_CONFIG.copy()
        self.load()

    def load(self):
        if CONFIG_PATH.exists():
            try:
                with open(CONFIG_PATH, "r") as f:
                    user_config = json.load(f)
                    self.config.update(user_config)
            except Exception as e:
                print(f"Error loading config: {e}")

    def save(self):
        CONFIG_PATH.parent.mkdir(parents=True, exist_ok=True)
        try:
            with open(CONFIG_PATH, "w") as f:
                json.dump(self.config, f, indent=4)
        except Exception as e:
            print(f"Error saving config: {e}")

    def get(self, key, default=None):
        return self.config.get(key, default if default is not None else DEFAULT_CONFIG.get(key))

    def set(self, key, value):
        self.config[key] = value
        self.save()
