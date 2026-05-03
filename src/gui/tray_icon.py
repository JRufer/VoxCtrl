from PyQt6.QtWidgets import QSystemTrayIcon, QMenu, QApplication
from PyQt6.QtGui import QIcon, QAction, QColor, QPixmap
from PyQt6.QtCore import QObject
import os

class WhisperTrayIcon(QSystemTrayIcon):
    def __init__(self, app_state):
        super().__init__()
        self.app_state = app_state
        
        # Resolve absolute paths to assets
        # __file__ is /.../src/gui/tray_icon.py
        # assets is /.../assets/
        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        assets_dir = os.path.join(base_dir, "assets")
        
        self.icon_on = QIcon(os.path.join(assets_dir, "record_on.png"))
        self.icon_off = QIcon(os.path.join(assets_dir, "record_off.png"))
        
        self.set_idle_icon()
        self.setToolTip("Whisper Wayland")
        
        self.menu = QMenu()
        self.settings_action = QAction("Settings")
        self.quit_action = QAction("Quit")
        self.quit_action.triggered.connect(QApplication.instance().quit)
        
        self.menu.addAction(self.settings_action)
        self.menu.addSeparator()
        self.menu.addAction(self.quit_action)
        self.setContextMenu(self.menu)

        self.app_state.recording_started.connect(self.set_recording_icon)
        self.app_state.recording_stopped.connect(self.set_idle_icon)

    def set_idle_icon(self):
        if hasattr(self, 'icon_off') and not self.icon_off.isNull():
            self.setIcon(self.icon_off)
        else:
            pixmap = QPixmap(64, 64)
            pixmap.fill(QColor("grey"))
            self.setIcon(QIcon(pixmap))

    def set_recording_icon(self):
        if hasattr(self, 'icon_on') and not self.icon_on.isNull():
            self.setIcon(self.icon_on)
        else:
            pixmap = QPixmap(64, 64)
            pixmap.fill(QColor("red"))
            self.setIcon(QIcon(pixmap))
