import shutil
import threading
import queue
import time
import subprocess
import os

# Word count accumulator for P0.6 (session stats)
_session_word_count = 0

class TextInjector(threading.Thread):
    def __init__(self, config, text_queue, word_count_callback=None):
        super().__init__(daemon=True)
        self.config = config
        self.text_queue = text_queue
        self.word_count_callback = word_count_callback  # P0.6: called with new total
        self.running = True
        self._session_words = 0

    def _send_notification(self, text):
        """P0.5: Fire a desktop notification via notify-send (best-effort)."""
        if not self.config.get("show_notification", False):
            return
        if not shutil.which("notify-send"):
            return
        preview = (text[:60] + "…") if len(text) > 60 else text
        subprocess.Popen(
            ["notify-send", "-t", "3000", "-i", "audio-input-microphone",
             "Whisper Wayland", preview],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )

    def _env(self):
        env = os.environ.copy()
        # Auto-detect Wayland socket if WAYLAND_DISPLAY isn't set
        if 'WAYLAND_DISPLAY' not in env:
            uid = os.getuid()
            for i in range(3):
                if os.path.exists(f"/run/user/{uid}/wayland-{i}"):
                    env['WAYLAND_DISPLAY'] = f"wayland-{i}"
                    break
        return env

    def inject_text(self, text):
        if not text:
            return

        print(f"Injecting: {text}")
        env = self._env()
        is_wayland = 'WAYLAND_DISPLAY' in env
        is_x11 = 'DISPLAY' in env
        injected = False

        # --- Wayland: wtype types directly, no clipboard needed ---
        if is_wayland and shutil.which('wtype'):
            result = subprocess.run(
                ['wtype', '--', text], env=env,
                stderr=subprocess.DEVNULL
            )
            if result.returncode == 0:
                injected = True

        # --- X11: xdotool types directly, no clipboard needed ---
        if not injected and is_x11 and shutil.which('xdotool'):
            result = subprocess.run(
                ['xdotool', 'type', '--clearmodifiers', '--delay', '12', '--', text],
                env=env, stderr=subprocess.DEVNULL
            )
            if result.returncode == 0:
                injected = True

        # --- Clipboard + paste fallback ---
        if not injected:
            copied = False
            if is_wayland and shutil.which('wl-copy'):
                proc = subprocess.run(
                    ['wl-copy'], input=text.encode('utf-8'),
                    env=env, stderr=subprocess.DEVNULL
                )
                copied = proc.returncode == 0

            if not copied and is_x11 and shutil.which('xclip'):
                proc = subprocess.run(
                    ['xclip', '-selection', 'clipboard'],
                    input=text.encode('utf-8'), env=env,
                    stderr=subprocess.DEVNULL
                )
                copied = proc.returncode == 0

            if not copied:
                try:
                    import pyperclip
                    pyperclip.copy(text)
                    copied = True
                except Exception:
                    pass

            if copied:
                time.sleep(0.05)
                if is_wayland and shutil.which('ydotool'):
                    subprocess.run(['ydotool', 'key', 'ctrl+v'], env=env, stderr=subprocess.DEVNULL)
                elif is_x11 and shutil.which('xdotool'):
                    subprocess.run(['xdotool', 'key', 'ctrl+v'], env=env, stderr=subprocess.DEVNULL)
                injected = True

        if injected:
            # P0.5: desktop notification
            self._send_notification(text)
            # P0.6: session word count
            self._session_words += len(text.split())
            if self.word_count_callback:
                self.word_count_callback(self._session_words)
        else:
            print("All injection methods failed.")

    @property
    def session_word_count(self):
        """P0.6: Total words injected this session."""
        return self._session_words


    def run(self):
        while self.running:
            try:
                text = self.text_queue.get(timeout=0.1)
                self.inject_text(text)
            except queue.Empty:
                continue
            except Exception as e:
                print(f"Error in text injection: {e}")

    def stop(self):
        self.running = False
