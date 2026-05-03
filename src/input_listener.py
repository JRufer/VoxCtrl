import evdev
from evdev import ecodes
import threading
import time
import os

class InputListener(threading.Thread):
    def __init__(self, config, on_press, on_release):
        super().__init__(daemon=True)
        self.config = config
        self.on_press = on_press
        self.on_release = on_release
        self.device = None
        self.target_keys = set()
        self.target_toggle_keys = set()
        self.hotkey_names = []
        self.toggle_hotkey_names = []
        self.pressed_keys = set()
        self.running = True
        self.toggle_state = False
        self.last_toggle_match = False

    _VIRTUAL_DEVICE_NAMES = ("whisper-wayland", "passthrough", "uinput", "virtual")

    def _is_virtual(self, dev):
        name_lower = dev.name.lower()
        return any(kw in name_lower for kw in self._VIRTUAL_DEVICE_NAMES)

    def find_device(self):
        saved_path = self.config.get("evdev_device")
        if saved_path and os.path.exists(saved_path):
            try:
                dev = evdev.InputDevice(saved_path)
                if not self._is_virtual(dev):
                    return dev
            except Exception:
                pass

        # Sort by event number ascending — physical hardware has lower numbers
        paths = sorted(evdev.list_devices(), key=lambda p: int(p.replace("/dev/input/event", "") or 0))
        for path in paths:
            try:
                dev = evdev.InputDevice(path)
            except Exception:
                continue
            if self._is_virtual(dev):
                continue
            if ecodes.EV_KEY in dev.capabilities():
                if ecodes.KEY_A in dev.capabilities()[ecodes.EV_KEY]:
                    print(f"Automatically selected input device: {dev.name} ({dev.path})")
                    return dev
        return None

    def update_hotkey(self):
        self.hotkey_names = self.config.get("hotkey", ["KEY_LEFTMETA", "KEY_SPACE"])
        self.toggle_hotkey_names = self.config.get("toggle_hotkey", ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"])
        
        print(f"Updating hotkeys: Hold={self.hotkey_names}, Toggle={self.toggle_hotkey_names}")
        
        self.target_keys = {ecodes.ecodes[name] for name in self.hotkey_names}
        self.target_toggle_keys = {ecodes.ecodes[name] for name in self.toggle_hotkey_names}
        
        self.pressed_keys.clear()

    def update_device(self):
        print("Triggering input device re-selection...")
        if self.device and self.device.path != self.config.get("evdev_device"):
            try:
                self.device.close()
            except:
                pass
            self.device = None

    def run(self):
        self.update_hotkey()

        while self.running:
            try:
                # Close any previously opened device before re-finding (prevents fd leak)
                if self.device:
                    try:
                        self.device.close()
                    except Exception:
                        pass
                    self.device = None

                self.device = self.find_device()
                if not self.device:
                    print("No suitable input device found. Retrying in 5s...")
                    time.sleep(5)
                    continue

                print(f"[*] Listening on {self.device.path}")
                is_holding_active = False
                is_toggle_active = False

                for event in self.device.read_loop():
                    if not self.running:
                        break

                    if event.type == ecodes.EV_KEY:
                        key_event = evdev.categorize(event)
                        scancode = key_event.scancode

                        if key_event.keystate == evdev.KeyEvent.key_down:
                            self.pressed_keys.add(scancode)
                        elif key_event.keystate == evdev.KeyEvent.key_up:
                            self.pressed_keys.discard(scancode)

                        # Check Hold Hotkey
                        is_hold_match = self.target_keys.issubset(self.pressed_keys)
                        if is_hold_match and not is_holding_active:
                            is_holding_active = True
                            if not is_toggle_active:
                                self.on_press()
                        elif not is_hold_match and is_holding_active:
                            is_holding_active = False
                            if not is_toggle_active:
                                self.on_release()

                        # Check Toggle Hotkey (edge-triggered)
                        is_toggle_match = self.target_toggle_keys.issubset(self.pressed_keys)
                        if is_toggle_match and not self.last_toggle_match:
                            self.toggle_state = not self.toggle_state
                            print(f"[*] Toggle State: {self.toggle_state}")
                            if self.toggle_state:
                                if not is_holding_active:
                                    self.on_press()
                            else:
                                if not is_holding_active:
                                    self.on_release()

                        self.last_toggle_match = is_toggle_match
            except Exception as e:
                print(f"Error in input listener: {e}")
                time.sleep(1)

    def stop(self):
        self.running = False
