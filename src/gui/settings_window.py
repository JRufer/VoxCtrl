import pyaudio
from PyQt6.QtWidgets import (QWidget, QVBoxLayout, QHBoxLayout, QLabel,
                             QComboBox, QLineEdit, QPushButton, QMessageBox, QSlider, QCheckBox, QTextEdit)
from PyQt6.QtCore import pyqtSignal, Qt
import evdev
from evdev import ecodes

class SettingsWindow(QWidget):
    settings_saved = pyqtSignal()

    def __init__(self, config, inference_engine=None):
        super().__init__()
        self.config = config
        self.inference_engine = inference_engine
        self.setWindowTitle("Whisper Wayland Settings")
        self.setMinimumWidth(400)
        self.setLayout(QVBoxLayout())
        
        # Set Window Icon
        import os
        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        icon_path = os.path.join(base_dir, "assets", "app_icon.png")
        if os.path.exists(icon_path):
            from PyQt6.QtGui import QIcon
            self.setWindowIcon(QIcon(icon_path))
        
   
        # Model Selection
        self.layout().addWidget(QLabel("Whisper Model:"))
        self.model_combo = QComboBox()
        self.model_combo.addItems(["tiny", "base", "small", "medium", "large-v3"])
        self.model_combo.setCurrentText(self.config.get("model_size", "base"))
        self.layout().addWidget(self.model_combo)
        
        # Audio Device Selection
        self.layout().addWidget(QLabel("Audio Input Device:"))
        self.device_combo = QComboBox()
        self.populate_audio_devices()
        self.layout().addWidget(self.device_combo)

        # Inference Mode
        self.layout().addWidget(QLabel("Inference Real-Time Mode:"))
        self.inference_mode_combo = QComboBox()
        self.inference_mode_combo.addItems(["Balanced", "Aggressive"])
        self.inference_mode_combo.setCurrentText(self.config.get("inference_mode", "Balanced"))
        self.layout().addWidget(self.inference_mode_combo)
        
        # Inference Device Selection
        self.layout().addWidget(QLabel("Inference Device:"))
        self.device_type_combo = QComboBox()
        self.device_type_combo.addItems(["auto", "cuda", "cpu"])
        self.device_type_combo.setCurrentText(self.config.get("device", "auto"))
        self.layout().addWidget(self.device_type_combo)

        # Status Label
        if self.inference_engine:
            status_text = f"Active: {self.config.get('model_size')} on {self.inference_engine.actual_device} ({self.inference_engine.actual_compute_type})"
        else:
            status_text = "Status: Engine not connected"
            
        self.status_label = QLabel(status_text)
        self.status_label.setStyleSheet("color: #666; font-style: italic; margin-bottom: 10px;")
        self.layout().addWidget(self.status_label)

        # Keyboard Device Selection (evdev)
        self.layout().addWidget(QLabel("Keyboard Event Device (evdev):"))
        self.kbd_combo = QComboBox()
        self.populate_keyboard_devices()
        self.layout().addWidget(self.kbd_combo)
        
        # Overlay Toggle
        self.overlay_checkbox = QCheckBox("Display Real-Time Overlay")
        self.overlay_checkbox.setChecked(self.config.get("show_overlay", True))
        self.layout().addWidget(self.overlay_checkbox)

        # Microphone Boost (Gain)
        self.layout().addWidget(QLabel("Microphone Boost (Software Gain):"))
        gain_layout = QHBoxLayout()
        self.gain_slider = QSlider(Qt.Orientation.Horizontal)
        self.gain_slider.setRange(5, 50) # 0.5x to 5.0x
        # Value in config is float, e.g. 1.0. Slider uses ints.
        current_gain = self.config.get("mic_gain", 1.0)
        self.gain_slider.setValue(int(current_gain * 10))
        
        self.gain_label = QLabel(f"{current_gain:.1f}x")
        self.gain_slider.valueChanged.connect(lambda v: self.gain_label.setText(f"{v/10.0:.1f}x"))
        
        gain_layout.addWidget(self.gain_slider)
        gain_layout.addWidget(self.gain_label)
        self.layout().addLayout(gain_layout)
        
        # Filler Word Removal
        self.filler_checkbox = QCheckBox("Remove filler words (um, uh, hmm, er, ah)")
        self.filler_checkbox.setChecked(self.config.get("remove_fillers", True))
        self.layout().addWidget(self.filler_checkbox)

        # Custom Vocabulary
        self.layout().addWidget(QLabel("Custom Vocabulary (comma-separated words/phrases):"))
        self.vocab_edit = QTextEdit()
        self.vocab_edit.setPlaceholderText("e.g. PyQt6, Wayland, kubernetes, Josh Rufer")
        self.vocab_edit.setFixedHeight(64)
        vocab_list = self.config.get("custom_vocabulary", [])
        self.vocab_edit.setPlainText(", ".join(vocab_list))
        self.layout().addWidget(self.vocab_edit)

        # Hold Hotkey Selection
        self.layout().addWidget(QLabel("Global 'Hold-to-Talk' Hotkey:"))
        h_layout = QHBoxLayout()
        self.hotkey_label = QLabel(", ".join(self.config.get("hotkey", ["KEY_LEFTMETA", "KEY_SPACE"])))
        self.hotkey_label.setStyleSheet("font-weight: bold; border: 1px solid #ccc; padding: 5px;")
        h_layout.addWidget(self.hotkey_label)
        
        self.record_btn_hold = QPushButton("Record")
        self.record_btn_hold.setCheckable(True)
        self.record_btn_hold.clicked.connect(lambda: self.toggle_recording("hold"))
        h_layout.addWidget(self.record_btn_hold)
        self.layout().addLayout(h_layout)

        # Toggle Hotkey Selection
        self.layout().addWidget(QLabel("Global 'Toggle-to-Talk' Hotkey:"))
        t_layout = QHBoxLayout()
        self.toggle_hotkey_label = QLabel(", ".join(self.config.get("toggle_hotkey", ["KEY_LEFTCTRL", "KEY_LEFTMETA", "KEY_SPACE"])))
        self.toggle_hotkey_label.setStyleSheet("font-weight: bold; border: 1px solid #ccc; padding: 5px;")
        t_layout.addWidget(self.toggle_hotkey_label)
        
        self.record_btn_toggle = QPushButton("Record")
        self.record_btn_toggle.setCheckable(True)
        self.record_btn_toggle.clicked.connect(lambda: self.toggle_recording("toggle"))
        t_layout.addWidget(self.record_btn_toggle)
        self.layout().addLayout(t_layout)
        
        # Recording state
        self.recorded_keys = set()
        self.recorded_toggle_keys = set()
        self.active_recording_mode = None # "hold" or "toggle"
        
        # System Check
        self.layout().addWidget(QLabel("System Troubleshooting:"))
        self.check_btn = QPushButton("Run Component & Permission Check")
        self.check_btn.setStyleSheet("background-color: #f0f0f0; margin-top: 10px;")
        self.check_btn.clicked.connect(self.run_system_check)
        self.layout().addWidget(self.check_btn)
        
        # Save/Cancel
        buttons = QHBoxLayout()
        save_btn = QPushButton("Save")
        save_btn.clicked.connect(self.save_settings)
        buttons.addWidget(save_btn)
        
        cancel_btn = QPushButton("Cancel")
        cancel_btn.clicked.connect(self.close)
        buttons.addWidget(cancel_btn)
        self.layout().addLayout(buttons)

    def run_system_check(self):
        import shutil
        import os
        import stat
        import grp
        
        report = []
        all_ok = True
        
        # 1. uinput
        uinput_path = "/dev/uinput"
        if os.path.exists(uinput_path):
            if os.access(uinput_path, os.W_OK):
                report.append("✅ /dev/uinput: Writable")
            else:
                report.append("❌ /dev/uinput: Permission denied")
                all_ok = False
        else:
            report.append("❌ /dev/uinput: Device not found")
            all_ok = False
            
        # 2. input group
        try:
            current_user = os.getlogin()
            user_groups = [grp.getgrgid(g).gr_name for g in os.getgroups()]
            if "input" in user_groups:
                report.append("✅ Group Membership: User in 'input' group")
            else:
                report.append("❌ Group Membership: User NOT in 'input' group")
                all_ok = False
        except Exception:
            report.append("⚠️ Group Membership: Could not verify")
            
        # 3. wl-clipboard
        if shutil.which("wl-copy"):
            report.append("✅ wl-clipboard: Installed (wl-copy found)")
        else:
            report.append("❌ wl-clipboard: Not found (Required for Wayland injection)")
            all_ok = False
            
        # 4. nvidia libs (check if we found them in inference engine)
        if self.inference_engine and self.inference_engine.actual_device == "cuda":
            report.append("✅ GPU Acceleration (CUDA): Active")
        elif self.config.get("device") == "cuda":
            report.append("⚠️ GPU Acceleration: Selected (Require restart to verify)")

        status_msg = "\n".join(report)
        if all_ok:
            QMessageBox.information(self, "System Check", f"System integration looks correct!\n\n{status_msg}")
        else:
            fix_msg = "\n\nSuggested Fix for permissions:\nsudo usermod -aG input $USER\n(Note: You must log out and back in for this to take effect)"
            QMessageBox.warning(self, "System Check", f"Issues detected:\n\n{status_msg}{fix_msg}")


    def populate_audio_devices(self):
        p = pyaudio.PyAudio()
        current_index = self.config.get("input_device_index")
        
        found_current = False
        for i in range(p.get_device_count()):
            info = p.get_device_info_by_index(i)
            if info.get('maxInputChannels') > 0:
                name = info.get('name')
                self.device_combo.addItem(f"{i}: {name}", i)
                if current_index == i:
                    self.device_combo.setCurrentIndex(self.device_combo.count() - 1)
                    found_current = True
        
        p.terminate()

    def populate_keyboard_devices(self):
        current_path = self.config.get("evdev_device")
        devices = [evdev.InputDevice(path) for path in evdev.list_devices()]
        
        # Sort so keyboards are at the top
        devices.sort(key=lambda d: (ecodes.EV_KEY in d.capabilities() and ecodes.KEY_A in d.capabilities().get(ecodes.EV_KEY, [])), reverse=True)
        
        for dev in devices:
            if ecodes.EV_KEY in dev.capabilities():
                self.kbd_combo.addItem(f"{dev.name} ({dev.path})", dev.path)
                if current_path == dev.path:
                    self.kbd_combo.setCurrentIndex(self.kbd_combo.count() - 1)

    def toggle_recording(self, mode):
        btn = self.record_btn_hold if mode == "hold" else self.record_btn_toggle
        
        if btn.isChecked():
            # If the other one was recording, stop it
            if self.active_recording_mode and self.active_recording_mode != mode:
                (self.record_btn_hold if self.active_recording_mode == "hold" else self.record_btn_toggle).setChecked(False)
            
            self.active_recording_mode = mode
            if mode == "hold":
                self.recorded_keys = set()
                self.hotkey_label.setText("Press... (Wait 1s after release)")
            else:
                self.recorded_toggle_keys = set()
                self.toggle_hotkey_label.setText("Press... (Wait 1s after release)")
            
            btn.setText("Stop")
            self.grabKeyboard()
        else:
            self.stop_recording()

    def stop_recording(self):
        self.active_recording_mode = None
        self.record_btn_hold.setChecked(False)
        self.record_btn_hold.setText("Record")
        self.record_btn_toggle.setChecked(False)
        self.record_btn_toggle.setText("Record")
        self.releaseKeyboard()
        
        if not self.recorded_keys:
            self.hotkey_label.setText(", ".join(self.config.get("hotkey")))
        if not self.recorded_toggle_keys:
            self.toggle_hotkey_label.setText(", ".join(self.config.get("toggle_hotkey")))

    def keyPressEvent(self, event):
        if self.active_recording_mode:
            key = event.key()
            mapping = {
                Qt.Key.Key_Meta: "KEY_LEFTMETA",
                Qt.Key.Key_Alt: "KEY_LEFTALT",
                Qt.Key.Key_Control: "KEY_LEFTCTRL",
                Qt.Key.Key_Shift: "KEY_LEFTSHIFT",
                Qt.Key.Key_Space: "KEY_SPACE",
                Qt.Key.Key_Enter: "KEY_ENTER",
                Qt.Key.Key_Return: "KEY_ENTER",
            }
            
            if Qt.Key.Key_A <= key <= Qt.Key.Key_Z:
                key_name = f"KEY_{chr(key)}"
            else:
                key_name = mapping.get(key)
                if not key_name:
                    key_name = f"KEY_{event.text().upper()}" if event.text() else None
            
            if key_name:
                if self.active_recording_mode == "hold":
                    self.recorded_keys.add(key_name)
                    self.hotkey_label.setText(", ".join(sorted(list(self.recorded_keys))))
                else:
                    self.recorded_toggle_keys.add(key_name)
                    self.toggle_hotkey_label.setText(", ".join(sorted(list(self.recorded_toggle_keys))))
        else:
            super().keyPressEvent(event)

    def save_settings(self):
        self.config.set("model_size", self.model_combo.currentText())
        
        # Audio device
        device_index = self.device_combo.currentData()
        self.config.set("input_device_index", device_index)

        # Keyboard device
        kbd_path = self.kbd_combo.currentData()
        self.config.set("evdev_device", kbd_path)
        
        # Hotkeys
        if self.recorded_keys:
            self.config.set("hotkey", sorted(list(self.recorded_keys)))
        if self.recorded_toggle_keys:
            self.config.set("toggle_hotkey", sorted(list(self.recorded_toggle_keys)))
        
        # Inference Mode
        self.config.set("inference_mode", self.inference_mode_combo.currentText())
        
        # Gain
        self.config.set("mic_gain", self.gain_slider.value() / 10.0)
        
        # Overlay
        self.config.set("show_overlay", self.overlay_checkbox.isChecked())

        # Filler removal
        self.config.set("remove_fillers", self.filler_checkbox.isChecked())

        # Custom vocabulary — split on commas, strip whitespace, drop empties
        raw = self.vocab_edit.toPlainText()
        vocab = [w.strip() for w in raw.split(",") if w.strip()]
        self.config.set("custom_vocabulary", vocab)
        
        # Inference Device & Auto-Compute
        new_device = self.device_type_combo.currentText()
        self.config.set("device", new_device)
        
        if new_device == "cpu":
            self.config.set("compute_type", "int8")
        elif new_device == "cuda":
            self.config.set("compute_type", "float16")
        else: # auto
            self.config.set("compute_type", "default")
        
        self.config.save()
        self.settings_saved.emit()
        QMessageBox.information(self, "Settings", "Settings saved! A restart is required to apply changes to the model.")
        self.close()
