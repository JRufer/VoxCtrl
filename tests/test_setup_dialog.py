"""Tests for src/gui/setup_dialog.py permission-detection helpers.

The pure helper functions (can_access_input_devices, user_in_input_group,
udev_rule_exists, needs_setup) run without Qt and are tested with mocks.

The PermissionsSetupDialog smoke-test is gated on both PyQt6 being importable
and a display being present; skipped gracefully in headless CI.
"""

import grp
import os
import pwd
import sys
import tempfile
import unittest.mock as mock
from pathlib import Path

import pytest

# Skip this entire module if PyQt6 is not installed (e.g. bare CI without GUI deps).
pytest.importorskip("PyQt6")

sys.path.insert(0, os.path.join(os.path.dirname(__file__), "..", "src"))

import gui.setup_dialog as sd


# ── can_access_input_devices ──────────────────────────────────────────────────

class TestCanAccessInputDevices:

    def test_returns_true_when_device_opens(self):
        with mock.patch("glob.glob", return_value=["/dev/input/event0"]), \
             mock.patch("builtins.open", mock.mock_open()):
            assert sd.can_access_input_devices() is True

    def test_returns_false_on_permission_error(self):
        with mock.patch("glob.glob", return_value=["/dev/input/event0"]), \
             mock.patch("builtins.open", side_effect=PermissionError):
            assert sd.can_access_input_devices() is False

    def test_returns_true_when_no_devices_exist(self):
        # No /dev/input/event* → likely a VM; startup must not be blocked.
        with mock.patch("glob.glob", return_value=[]):
            assert sd.can_access_input_devices() is True

    def test_skips_oserror_device_and_continues(self):
        # event0 is busy (OSError), event1 opens fine → True overall.
        cm = mock.MagicMock()

        def _open(path, *a, **kw):
            if "event0" in path:
                raise OSError("Device busy")
            return cm

        with mock.patch("glob.glob", return_value=["/dev/input/event0",
                                                    "/dev/input/event1"]), \
             mock.patch("builtins.open", side_effect=_open):
            assert sd.can_access_input_devices() is True

    def test_all_oserror_falls_through_to_true(self):
        # Every candidate raises OSError (not PermissionError) → no devices
        # accessible but not a permission problem, return True.
        with mock.patch("glob.glob", return_value=["/dev/input/event0"]), \
             mock.patch("builtins.open", side_effect=OSError("busy")):
            assert sd.can_access_input_devices() is True

    def test_checks_at_most_eight_devices(self):
        # The function slices glob results to [:8]. Simulate 20 paths; all
        # raise OSError so the loop completes without early return.
        tried = []

        def _open(path, *a, **kw):
            tried.append(path)
            raise OSError("busy")

        paths = [f"/dev/input/event{i}" for i in range(20)]
        with mock.patch("glob.glob", return_value=paths), \
             mock.patch("builtins.open", side_effect=_open):
            result = sd.can_access_input_devices()

        assert result is True        # all OSError → fallthrough
        assert len(tried) == 8       # sliced to [:8] before the loop

    def test_permission_error_short_circuits_remaining_devices(self):
        # PermissionError on the first device means we know access is denied —
        # no need to probe further.
        tried = []

        def _open(path, *a, **kw):
            tried.append(path)
            raise PermissionError

        paths = ["/dev/input/event0", "/dev/input/event1", "/dev/input/event2"]
        with mock.patch("glob.glob", return_value=paths), \
             mock.patch("builtins.open", side_effect=_open):
            assert sd.can_access_input_devices() is False

        assert len(tried) == 1   # returned immediately after first PermissionError


# ── user_in_input_group ───────────────────────────────────────────────────────

def _grp(gr_gid, gr_mem):
    return grp.struct_group(("input", "x", gr_gid, gr_mem))


def _pwd(pw_uid, pw_gid, pw_name="alice"):
    return pwd.struct_passwd((pw_name, "x", pw_uid, pw_gid, "", f"/home/{pw_name}", "/bin/bash"))


class TestUserInInputGroup:

    def test_returns_true_when_username_in_mem(self):
        with mock.patch("grp.getgrnam", return_value=_grp(990, ["alice", "bob"])), \
             mock.patch("pwd.getpwuid", return_value=_pwd(1000, 1000, "alice")), \
             mock.patch("pwd.getpwnam", return_value=_pwd(1000, 1000, "alice")), \
             mock.patch("os.getuid", return_value=1000):
            assert sd.user_in_input_group() is True

    def test_returns_false_when_username_not_in_mem(self):
        with mock.patch("grp.getgrnam", return_value=_grp(990, ["bob", "charlie"])), \
             mock.patch("pwd.getpwuid", return_value=_pwd(1000, 1000, "alice")), \
             mock.patch("pwd.getpwnam", return_value=_pwd(1000, 1000, "alice")), \
             mock.patch("os.getuid", return_value=1000):
            assert sd.user_in_input_group() is False

    def test_returns_true_when_primary_gid_matches(self):
        # User whose primary group IS the input group (pw_gid == gr_gid).
        with mock.patch("grp.getgrnam", return_value=_grp(990, [])), \
             mock.patch("pwd.getpwuid", return_value=_pwd(1000, 990, "alice")), \
             mock.patch("pwd.getpwnam", return_value=_pwd(1000, 990, "alice")), \
             mock.patch("os.getuid", return_value=1000):
            assert sd.user_in_input_group() is True

    def test_returns_false_when_group_does_not_exist(self):
        with mock.patch("grp.getgrnam", side_effect=KeyError("input")):
            assert sd.user_in_input_group() is False

    def test_returns_false_on_unexpected_error(self):
        with mock.patch("grp.getgrnam", side_effect=RuntimeError("unexpected")):
            assert sd.user_in_input_group() is False


# ── udev_rule_exists ──────────────────────────────────────────────────────────

class TestUdevRuleExists:

    def test_returns_true_when_file_present(self):
        with tempfile.NamedTemporaryFile() as f:
            with mock.patch.object(sd, "_UDEV_RULE_PATH", Path(f.name)):
                assert sd.udev_rule_exists() is True

    def test_returns_false_when_file_absent(self):
        with tempfile.TemporaryDirectory() as d:
            absent = Path(d) / "99-whisper-wayland.rules"
            with mock.patch.object(sd, "_UDEV_RULE_PATH", absent):
                assert sd.udev_rule_exists() is False


# ── needs_setup ───────────────────────────────────────────────────────────────

class TestNeedsSetup:

    def test_false_when_devices_are_accessible(self):
        with mock.patch.object(sd, "can_access_input_devices", return_value=True):
            assert sd.needs_setup() is False

    def test_true_when_devices_are_not_accessible(self):
        with mock.patch.object(sd, "can_access_input_devices", return_value=False):
            assert sd.needs_setup() is True


# ── PermissionsSetupDialog smoke-test ─────────────────────────────────────────
# Requires a display. Skipped automatically in headless environments.

_HAS_DISPLAY = bool(os.environ.get("DISPLAY") or os.environ.get("WAYLAND_DISPLAY"))


@pytest.mark.skipif(not _HAS_DISPLAY, reason="No display available (headless)")
class TestPermissionsSetupDialogSmoke:

    @pytest.fixture(scope="class")
    def qapp(self):
        from PyQt6.QtWidgets import QApplication
        app = QApplication.instance() or QApplication([])
        yield app

    def test_dialog_instantiates(self, qapp):
        dlg = sd.PermissionsSetupDialog()
        assert dlg is not None

    def test_already_configured_flag_true(self, qapp):
        with mock.patch.object(sd, "user_in_input_group", return_value=True), \
             mock.patch.object(sd, "udev_rule_exists", return_value=True):
            dlg = sd.PermissionsSetupDialog()
            assert dlg._already_configured is True

    def test_already_configured_flag_false(self, qapp):
        with mock.patch.object(sd, "user_in_input_group", return_value=False), \
             mock.patch.object(sd, "udev_rule_exists", return_value=False):
            dlg = sd.PermissionsSetupDialog()
            assert dlg._already_configured is False

    def test_partially_configured_flag_false(self, qapp):
        # One piece done, one missing → not "already configured" → show setup button
        with mock.patch.object(sd, "user_in_input_group", return_value=True), \
             mock.patch.object(sd, "udev_rule_exists", return_value=False):
            dlg = sd.PermissionsSetupDialog()
            assert dlg._already_configured is False
