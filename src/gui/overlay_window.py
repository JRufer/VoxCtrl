from PyQt6.QtWidgets import QWidget, QLabel, QVBoxLayout
from PyQt6.QtCore import Qt, QTimer
from PyQt6.QtGui import QColor, QFont

class OverlayWindow(QWidget):
    def __init__(self):
        super().__init__()
        
        # Frameless, Always on top, Tool window (no taskbar entry), Click-through
        # Use Frameless + Window
        self.setWindowFlags(
            Qt.WindowType.Window |
            Qt.WindowType.FramelessWindowHint |
            Qt.WindowType.WindowStaysOnTopHint
        )
        # self.setAttribute(Qt.WidgetAttribute.WA_TranslucentBackground)
        self.setAttribute(Qt.WidgetAttribute.WA_ShowWithoutActivating)
        
        layout = QVBoxLayout()
        self.label = QLabel("")
        self.label.setAlignment(Qt.AlignmentFlag.AlignCenter)
        self.label.setStyleSheet("""
            QLabel {
                color: white;
                background-color: rgba(0, 0, 0, 200);
                border: 1px solid #444444;
                border-radius: 15px;
                padding: 10px 20px;
                font-size: 18px;
                font-family: 'Inter', sans-serif;
                font-weight: 500;
            }
        """)
        layout.addWidget(self.label)
        self.setLayout(layout)
        
        # Center on screen (roughly)
        self.resize(800, 100)
        self.hide()

    def set_text(self, text, force_show=False):
        # If we aren't force showing (usual case for real-time updates), 
        # and we are hidden, don't show ourselves based on a late update.
        if not force_show and self.isHidden():
            return

        if not text:
            self.label.setText("...")
        else:
            self.label.setText(text)
        
        # Position at the very bottom of the screen
        screen = self.screen().geometry()
        x = (screen.width() - self.width()) // 2
        y = screen.height() - self.height() - 50 # 50px padding from bottom
        self.move(x, y)
        self.show()

    def clear_and_hide(self):
        self.label.setText("")
        self.hide()
