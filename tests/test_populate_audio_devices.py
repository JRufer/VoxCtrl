"""Tests for SettingsWindow.populate_audio_devices.

Verifies the fix for the SIGSEGV crash: when an audio_recorder is
present its PyAudio instance (recorder.p) must be reused for device
enumeration and terminate() must never be called on it, because
PortAudio does not reference-count Pa_Initialize/Pa_Terminate — a
stray terminate() globally tears down PortAudio and leaves recorder.p
in an invalid state that causes a SIGSEGV on the next Pa_*() call.

The SettingsWindow constructor is heavy (Qt widgets, threading), so
populate_audio_devices is exercised as an unbound method called on a
lightweight mock object that supplies only the attributes the method
reads: self.audio_recorder, self.config, and self.device_combo.
"""
import os
import sys
import types
import unittest.mock as mock

import pytest

# ── Stub the native packages that are absent in a headless test environment ────
#
# Only modules that settings_window.py imports at the *module level* and that
# are genuinely unavailable (pyaudio, evdev, PyQt6) are stubbed here.
# Project-internal modules (routing.loader, audio_recorder, …) are imported
# lazily inside __init__ or tab builders and are NOT touched, so they remain
# available for other test modules in the same pytest session.


class _AutoMockModule(types.ModuleType):
    """Module whose attributes auto-vend MagicMocks on first access."""
    def __getattr__(self, name):
        val = mock.MagicMock()
        setattr(self, name, val)  # cache so the same object is returned each time
        return val


def _stub(name, **attrs):
    m = _AutoMockModule(name)
    for k, v in attrs.items():
        setattr(m, k, v)
    sys.modules.setdefault(name, m)  # don't clobber if already present
    return sys.modules[name]


# pyaudio stub  (module-level `import pyaudio` in settings_window.py)
_pyaudio_mod = _stub("pyaudio", paInt16=8)

# evdev stubs  (module-level `import evdev` and `from evdev import ecodes`)
_ecodes_stub = _stub("evdev.ecodes", EV_KEY=1, KEY_A=30)
_evdev_stub = _stub("evdev",
                    list_devices=lambda: [],
                    InputDevice=mock.MagicMock,
                    ecodes=_ecodes_stub)

# PyQt6 stubs  (multiple `from PyQt6.QtX import …` at module level)
# Attributes resolve to MagicMock automatically, EXCEPT for names used as
# class bases: those must be real types so `class SettingsWindow(QWidget)`
# produces a genuine Python class rather than a MagicMock.
_QWidget_stub = type("QWidget", (object,), {})
_QDialog_stub = type("QDialog", (object,), {})

for _qname in ["PyQt6", "PyQt6.QtWidgets", "PyQt6.QtCore", "PyQt6.QtGui"]:
    _stub(_qname)

_qt_widgets_mod = sys.modules["PyQt6.QtWidgets"]
_qt_widgets_mod.QWidget = _QWidget_stub
_qt_widgets_mod.QDialog = _QDialog_stub

# numpy is imported at the top of audio_recorder.py which is imported inside
# some SettingsWindow methods, but not at module level of settings_window itself.
# We stub it here only so test_audio_recorder doesn't re-import a missing module
# if both test files load in the same session.
_stub("numpy")

# ── Import target ─────────────────────────────────────────────────────────────

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

from gui.settings_window import SettingsWindow  # noqa: E402


# ── Test helpers ──────────────────────────────────────────────────────────────

def _device_info(index, max_input_channels=1, name=None):
    """Dict resembling PyAudio's get_device_info_by_index output."""
    return {
        "maxInputChannels": max_input_channels,
        "name": name or f"Mic {index}",
    }


def _mock_pa(devices):
    """Mock PyAudio instance that enumerates a given device list."""
    pa = mock.MagicMock()
    pa.get_device_count.return_value = len(devices)
    pa.get_device_info_by_index.side_effect = lambda i: devices[i]
    return pa


def _host(recorder_pa=None, config_device_index=None, devices=None):
    """
    Minimal stand-in for 'self' inside populate_audio_devices.

    recorder_pa          – if given, wrapped in a mock audio_recorder (.p)
    config_device_index  – value returned by config.get("input_device_index")
    devices              – not used directly; caller controls recorder_pa's device list
    """
    h = mock.MagicMock()
    if recorder_pa is not None:
        h.audio_recorder = mock.MagicMock()
        h.audio_recorder.p = recorder_pa
    else:
        h.audio_recorder = None
    h.config.get.return_value = config_device_index
    h.device_combo = mock.MagicMock()
    h.device_combo.count.return_value = 0
    return h


# ── Tests: PyAudio instance selection ─────────────────────────────────────────

class TestPyAudioInstanceSelection:
    def test_reuses_recorder_pa_when_available(self):
        """recorder.p.get_device_count() is called — no fresh PyAudio created."""
        recorder_pa = _mock_pa([])
        h = _host(recorder_pa=recorder_pa)
        fresh_pa_cls = mock.MagicMock()
        _pyaudio_mod.PyAudio = fresh_pa_cls

        SettingsWindow.populate_audio_devices(h)

        recorder_pa.get_device_count.assert_called_once()
        fresh_pa_cls.assert_not_called()

    def test_does_not_terminate_recorder_pa(self):
        """The recorder's PyAudio must never have terminate() called on it."""
        recorder_pa = _mock_pa([])
        h = _host(recorder_pa=recorder_pa)

        SettingsWindow.populate_audio_devices(h)

        recorder_pa.terminate.assert_not_called()

    def test_creates_fresh_pa_when_no_recorder(self):
        """Without audio_recorder, a new PyAudio() is instantiated."""
        fresh_pa = _mock_pa([])
        fresh_pa_cls = mock.MagicMock(return_value=fresh_pa)
        _pyaudio_mod.PyAudio = fresh_pa_cls
        h = _host(recorder_pa=None)

        SettingsWindow.populate_audio_devices(h)

        fresh_pa_cls.assert_called_once()

    def test_terminates_fresh_pa_when_no_recorder(self):
        """The temporary PyAudio created without a recorder is terminated."""
        fresh_pa = _mock_pa([])
        _pyaudio_mod.PyAudio = mock.MagicMock(return_value=fresh_pa)
        h = _host(recorder_pa=None)

        SettingsWindow.populate_audio_devices(h)

        fresh_pa.terminate.assert_called_once()


# ── Tests: device combo population ───────────────────────────────────────────

class TestDeviceComboPopulation:
    def test_adds_input_capable_device(self):
        devices = [_device_info(0, max_input_channels=2, name="Built-in Mic")]
        pa = _mock_pa(devices)
        h = _host(recorder_pa=pa)

        SettingsWindow.populate_audio_devices(h)

        h.device_combo.addItem.assert_called_once_with("Built-in Mic", 0)

    def test_skips_output_only_device(self):
        devices = [_device_info(0, max_input_channels=0, name="Speakers")]
        pa = _mock_pa(devices)
        h = _host(recorder_pa=pa)

        SettingsWindow.populate_audio_devices(h)

        h.device_combo.addItem.assert_not_called()

    def test_adds_only_input_devices_from_mixed_list(self):
        devices = [
            _device_info(0, max_input_channels=0, name="Speakers"),
            _device_info(1, max_input_channels=1, name="USB Mic"),
            _device_info(2, max_input_channels=0, name="HDMI Out"),
            _device_info(3, max_input_channels=2, name="Headset"),
        ]
        pa = _mock_pa(devices)
        h = _host(recorder_pa=pa)

        SettingsWindow.populate_audio_devices(h)

        added = [c.args for c in h.device_combo.addItem.call_args_list]
        assert ("USB Mic", 1) in added
        assert ("Headset", 3) in added
        assert len(added) == 2

    def test_selects_configured_device_index(self):
        devices = [
            _device_info(0, name="Default Mic"),
            _device_info(1, name="USB Mic"),
        ]
        pa = _mock_pa(devices)
        # Simulate combo.count() incrementing as items are added.
        count = [0]
        h = _host(recorder_pa=pa, config_device_index=1)
        h.device_combo.addItem.side_effect = lambda *_: count.__setitem__(0, count[0] + 1)
        h.device_combo.count.side_effect = lambda: count[0]

        SettingsWindow.populate_audio_devices(h)

        h.device_combo.setCurrentIndex.assert_called_once()

    def test_no_selection_when_configured_index_absent(self):
        devices = [_device_info(0, name="Mic A")]
        pa = _mock_pa(devices)
        h = _host(recorder_pa=pa, config_device_index=99)

        SettingsWindow.populate_audio_devices(h)

        h.device_combo.setCurrentIndex.assert_not_called()

    def test_empty_device_list(self):
        pa = _mock_pa([])
        h = _host(recorder_pa=pa)

        SettingsWindow.populate_audio_devices(h)

        h.device_combo.addItem.assert_not_called()
        pa.terminate.assert_not_called()
