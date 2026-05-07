"""
P1.2 — Transcription History Panel

Shows a timestamped, searchable log of every transcription this session.
Optionally persists to ~/.local/share/voxctl/history.jsonl.
"""
import json
import os
import subprocess
from datetime import datetime
from pathlib import Path

from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout, QLabel, QLineEdit,
    QPushButton, QScrollArea, QFrame, QSizePolicy, QCheckBox,
    QApplication,
)
from PyQt6.QtCore import Qt, pyqtSignal, QTimer
from PyQt6.QtGui import QIcon, QFont, QClipboard

HISTORY_PATH = Path.home() / ".local" / "share" / "voxctl" / "history.jsonl"

QSS = """
QWidget {
    background-color: #0f1117;
    color: #e2e8f0;
    font-family: 'Segoe UI', 'Inter', 'Ubuntu', sans-serif;
    font-size: 13px;
}
QLineEdit {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 7px 10px;
    color: #e2e8f0;
}
QLineEdit:focus { border-color: #4a9eff; }
QPushButton {
    background: #1a1f2e;
    border: 1px solid #2a3448;
    border-radius: 6px;
    padding: 6px 14px;
    color: #c8d3e0;
}
QPushButton:hover { background: #242b3d; border-color: #4a9eff; }
QPushButton:pressed { background: #4a9eff; color: #fff; }
QPushButton#btn_danger {
    color: #f87171;
    border-color: #3a1f1f;
}
QPushButton#btn_danger:hover { background: #3a1f1f; }
QScrollArea { border: none; }
QCheckBox { spacing: 8px; color: #8892a4; }
QCheckBox::indicator {
    width: 16px; height: 16px;
    border: 1px solid #2a3448;
    border-radius: 3px;
    background: #1a1f2e;
}
QCheckBox::indicator:checked {
    background: #4a9eff;
    border-color: #4a9eff;
}
"""


class HistoryEntry(QFrame):
    """A single history card: timestamp + text + copy button."""

    def __init__(self, timestamp: str, text: str, parent=None):
        super().__init__(parent)
        self.text = text
        self.setFrameShape(QFrame.Shape.StyledPanel)
        self.setStyleSheet(
            "QFrame { background: #1a1f2e; border: 1px solid #1e2433;"
            " border-radius: 8px; padding: 0; }"
            "QFrame:hover { border-color: #2a3448; }"
        )

        outer = QVBoxLayout(self)
        outer.setContentsMargins(14, 10, 14, 10)
        outer.setSpacing(6)

        # Header row: timestamp + copy button
        header = QHBoxLayout()
        ts_label = QLabel(timestamp)
        ts_label.setStyleSheet("color: #4a5568; font-size: 11px; background: transparent; border: none;")
        header.addWidget(ts_label)
        header.addStretch()

        copy_btn = QPushButton("Copy")
        copy_btn.setFixedHeight(24)
        copy_btn.setStyleSheet(
            "QPushButton { background: transparent; border: 1px solid #2a3448;"
            " border-radius: 4px; padding: 2px 10px; color: #4a9eff; font-size: 11px; }"
            "QPushButton:hover { background: #4a9eff22; }"
        )
        copy_btn.clicked.connect(self._copy)
        header.addWidget(copy_btn)
        outer.addLayout(header)

        # Text body
        body = QLabel(text)
        body.setWordWrap(True)
        body.setStyleSheet(
            "color: #e2e8f0; font-size: 13px; background: transparent; border: none;"
        )
        body.setSizePolicy(QSizePolicy.Policy.Expanding, QSizePolicy.Policy.Preferred)
        outer.addWidget(body)

    def _copy(self):
        QApplication.clipboard().setText(self.text)

    def matches(self, query: str) -> bool:
        return query.lower() in self.text.lower()


class HistoryWindow(QWidget):
    """P1.2 — Transcription history panel."""

    def __init__(self, config, parent=None):
        super().__init__(parent)
        self.config = config
        self._entries: list[HistoryEntry] = []

        self.setWindowTitle("VoxCtl — History")
        self.setMinimumWidth(540)
        self.setMinimumHeight(500)
        self.setStyleSheet(QSS)

        base_dir = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
        icon_path = os.path.join(base_dir, "assets", "app_icon.png")
        if os.path.exists(icon_path):
            self.setWindowIcon(QIcon(icon_path))

        root = QVBoxLayout(self)
        root.setContentsMargins(16, 16, 16, 16)
        root.setSpacing(10)

        # ── Header ────────────────────────────────────────────────────────
        header_row = QHBoxLayout()
        title = QLabel("📋  Transcription History")
        title.setFont(QFont("Segoe UI", 15, QFont.Weight.Bold))
        title.setStyleSheet("color: #e2e8f0;")
        header_row.addWidget(title)
        header_row.addStretch()
        root.addLayout(header_row)

        # ── Toolbar ───────────────────────────────────────────────────────
        toolbar = QHBoxLayout()
        self.search_box = QLineEdit()
        self.search_box.setPlaceholderText("🔍  Search transcriptions…")
        self.search_box.textChanged.connect(self._filter)
        toolbar.addWidget(self.search_box, 1)

        self.persist_cb = QCheckBox("Save to disk")
        self.persist_cb.setToolTip(str(HISTORY_PATH))
        toolbar.addWidget(self.persist_cb)

        clear_btn = QPushButton("Clear session")
        clear_btn.setObjectName("btn_danger")
        clear_btn.clicked.connect(self._clear)
        toolbar.addWidget(clear_btn)
        root.addLayout(toolbar)

        # Word count badge
        self.count_label = QLabel("0 entries")
        self.count_label.setStyleSheet("color: #4a5568; font-size: 11px;")
        root.addWidget(self.count_label)

        # ── Scroll area for cards ─────────────────────────────────────────
        self.cards_widget = QWidget()
        self.cards_layout = QVBoxLayout(self.cards_widget)
        self.cards_layout.setContentsMargins(0, 0, 0, 0)
        self.cards_layout.setSpacing(8)
        self.cards_layout.addStretch()

        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(self.cards_widget)
        root.addWidget(scroll)

        self._scroll = scroll

    # ── Public API ────────────────────────────────────────────────────────
    def add_entry(self, text: str):
        """Called by main.py after every successful transcription."""
        if not text.strip():
            return

        ts = datetime.now().strftime("%a %b %d · %H:%M:%S")
        entry = HistoryEntry(ts, text)
        self._entries.append(entry)

        # Insert at position 0 so newest entries appear at the top
        self.cards_layout.insertWidget(0, entry)
        self._update_count()
        self._filter(self.search_box.text())

        # Scroll to top so the new entry is immediately visible
        QTimer.singleShot(50, lambda: self._scroll.verticalScrollBar().setValue(0))

        if self.persist_cb.isChecked():
            self._persist(ts, text)

    # ── Internal ──────────────────────────────────────────────────────────
    def _filter(self, query: str):
        for entry in self._entries:
            entry.setVisible(entry.matches(query) if query else True)

    def _clear(self):
        for entry in self._entries:
            self.cards_layout.removeWidget(entry)
            entry.deleteLater()
        self._entries.clear()
        self._update_count()

    def _update_count(self):
        n = len(self._entries)
        self.count_label.setText(f"{n} {'entry' if n == 1 else 'entries'} this session")

    def _persist(self, ts: str, text: str):
        try:
            HISTORY_PATH.parent.mkdir(parents=True, exist_ok=True)
            with open(HISTORY_PATH, "a", encoding="utf-8") as f:
                f.write(json.dumps({"date": datetime.now().strftime("%Y-%m-%d"),
                                    "time": ts, "text": text}) + "\n")
        except Exception as e:
            print(f"[History] Failed to persist: {e}")
