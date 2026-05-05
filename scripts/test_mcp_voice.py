#!/usr/bin/env python3
"""
Live integration test for the Whisper-Wayland MCP server.

Requires Whisper-Wayland to be running with mcp_server_enabled=true and
TTS enabled. Run with:

    python3 scripts/test_mcp_voice.py

Optional arguments:
    --socket PATH       Unix socket path (default: /tmp/whisper-wayland-mcp.sock)
    --prompt TEXT       Text to speak before recording (default: built-in prompt)
    --timeout SECONDS   Max seconds to wait for speech (default: 15)
    --no-echo           Skip speaking the transcript back
    --list-tools        Just print the tool list and exit

Flow:
    1. Connect and handshake with the MCP server
    2. Call get_status to confirm the server is idle
    3. Call speak_text with a voice prompt
    4. Poll get_status until TTS finishes
    5. Call transcribe_voice to capture the user's reply
    6. Print the transcript
    7. Call speak_text to echo the transcript back (unless --no-echo)
    8. Poll get_status until TTS finishes
"""

import argparse
import json
import os
import socket
import sys
import time

SOCKET_PATH = "/tmp/whisper-wayland-mcp.sock"

# ANSI colours — disabled automatically on non-TTYs
_USE_COLOR = sys.stdout.isatty()

def _c(code: str, text: str) -> str:
    return f"\033[{code}m{text}\033[0m" if _USE_COLOR else text

def ok(msg):    print(_c("32", f"  ✓  {msg}"))
def info(msg):  print(_c("36", f"  ·  {msg}"))
def warn(msg):  print(_c("33", f"  ⚠  {msg}"), file=sys.stderr)
def err(msg):   print(_c("31", f"  ✗  {msg}"), file=sys.stderr)
def head(msg):  print(_c("1;35", f"\n{msg}"))


# ── Low-level socket helpers ───────────────────────────────────────────────────

class MCPClient:
    def __init__(self, socket_path: str):
        self._path = socket_path
        self._sock: socket.socket | None = None
        self._next_id = 1
        self._buf = b""

    def connect(self):
        if not os.path.exists(self._path):
            raise FileNotFoundError(
                f"Socket not found: {self._path}\n"
                "Is Whisper-Wayland running with mcp_server_enabled=true?"
            )
        self._sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self._sock.settimeout(30.0)
        self._sock.connect(self._path)

    def close(self):
        if self._sock:
            try:
                self._sock.close()
            except Exception:
                pass
            self._sock = None

    def _send(self, obj: dict):
        self._sock.sendall((json.dumps(obj) + "\n").encode("utf-8"))

    def _recv_line(self) -> dict:
        while b"\n" not in self._buf:
            chunk = self._sock.recv(4096)
            if not chunk:
                raise ConnectionError("Server closed the connection")
            self._buf += chunk
        line, self._buf = self._buf.split(b"\n", 1)
        return json.loads(line.strip())

    def call(self, method: str, params: dict | None = None) -> dict:
        rpc_id = self._next_id
        self._next_id += 1
        self._send({"jsonrpc": "2.0", "id": rpc_id, "method": method, "params": params or {}})
        resp = self._recv_line()
        if resp.get("id") != rpc_id:
            raise ValueError(f"Unexpected response id: {resp}")
        if "error" in resp:
            raise RuntimeError(f"RPC error: {resp['error']}")
        return resp["result"]

    def notify(self, method: str, params: dict | None = None):
        self._send({"jsonrpc": "2.0", "method": method, "params": params or {}})

    # ── MCP helpers ────────────────────────────────────────────────────────────

    def initialize(self) -> dict:
        result = self.call("initialize")
        self.notify("notifications/initialized")
        return result

    def list_tools(self) -> list[dict]:
        return self.call("tools/list")["tools"]

    def get_status(self) -> dict:
        result = self.call("tools/call", {"name": "get_status", "arguments": {}})
        return json.loads(result["content"][0]["text"])

    def speak_text(self, text: str) -> str:
        result = self.call("tools/call", {"name": "speak_text", "arguments": {"text": text}})
        return result["content"][0]["text"]

    def transcribe_voice(self, timeout_seconds: float = 15.0) -> str:
        result = self.call(
            "tools/call",
            {"name": "transcribe_voice", "arguments": {"timeout_seconds": timeout_seconds}},
        )
        return result["content"][0]["text"]

    def wait_until_idle(self, poll_interval: float = 0.5, max_wait: float = 120.0):
        """Poll get_status until both recording=false and speaking=false."""
        deadline = time.time() + max_wait
        while time.time() < deadline:
            status = self.get_status()
            if not status.get("recording") and not status.get("speaking"):
                return
            time.sleep(poll_interval)
        raise TimeoutError("Server did not become idle within the timeout")


# ── Test runner ────────────────────────────────────────────────────────────────

def run_test(args: argparse.Namespace) -> int:
    client = MCPClient(args.socket)

    # ── Step 1: Connect ────────────────────────────────────────────────────────
    head("Step 1 · Connect")
    try:
        client.connect()
    except FileNotFoundError as exc:
        err(str(exc))
        return 1
    ok(f"Connected to {args.socket}")

    try:
        # ── Step 2: Handshake ──────────────────────────────────────────────────
        head("Step 2 · Handshake")
        info_result = client.initialize()
        ok(f"Server: {info_result['serverInfo']['name']} "
           f"v{info_result['serverInfo']['version']}  "
           f"(protocol {info_result['protocolVersion']})")

        # ── Step 3: List tools (optional) ──────────────────────────────────────
        if args.list_tools:
            head("Tools")
            for tool in client.list_tools():
                print(f"    • {tool['name']}")
            return 0

        # ── Step 4: Check initial status ───────────────────────────────────────
        head("Step 3 · Initial status")
        status = client.get_status()
        ok(f"recording={status['recording']}  speaking={status['speaking']}")
        if status["recording"]:
            warn("A recording is already in progress — waiting for it to finish…")
            client.wait_until_idle()
        if status["speaking"]:
            warn("TTS is currently playing — waiting for it to finish…")
            client.wait_until_idle()

        # ── Step 5: Speak prompt ───────────────────────────────────────────────
        head("Step 4 · Speak prompt")
        prompt = args.prompt or "Hello! I'm ready to listen. Please say something after the beep."
        info(f"Speaking: "{prompt}"")
        queued = client.speak_text(prompt)
        ok(f"TTS queued ({queued})")

        # Brief pause then poll — speak_text returns before audio starts
        time.sleep(0.5)
        info("Waiting for TTS to finish…")
        client.wait_until_idle(max_wait=60.0)
        ok("TTS finished")

        # ── Step 6: Record voice ───────────────────────────────────────────────
        head("Step 5 · Record voice")
        info(f"Listening for up to {args.timeout:.0f} seconds — speak now…")
        transcript = client.transcribe_voice(timeout_seconds=args.timeout)

        if transcript == "(no speech detected)":
            warn("No speech was detected within the timeout.")
            print()
            print("  Possible causes:")
            print("    • Microphone not selected or muted")
            print("    • VAD threshold too high (adjust in Settings → Audio)")
            print("    • timeout too short (use --timeout 30)")
            return 1

        ok("Transcription received")
        print()
        print(_c("1", "  Transcript:"), _c("32;1", f'"{transcript}"'))
        print()

        # ── Step 7: Echo transcript back ───────────────────────────────────────
        if not args.no_echo:
            head("Step 6 · Echo transcript via TTS")
            echo_text = f"I heard you say: {transcript}"
            info(f"Speaking: "{echo_text}"")
            client.speak_text(echo_text)
            ok("TTS queued — listening for playback to finish…")
            time.sleep(0.5)
            client.wait_until_idle(max_wait=120.0)
            ok("Playback complete")

        # ── Final status ───────────────────────────────────────────────────────
        head("Final status")
        status = client.get_status()
        ok(f"recording={status['recording']}  speaking={status['speaking']}")
        print()
        print(_c("32;1", "  All steps passed."))
        return 0

    except TimeoutError as exc:
        err(f"Timed out: {exc}")
        return 1
    except (ConnectionError, RuntimeError, ValueError) as exc:
        err(f"Error: {exc}")
        return 1
    finally:
        client.close()


# ── Entry point ────────────────────────────────────────────────────────────────

def main():
    parser = argparse.ArgumentParser(
        description="Live integration test for the Whisper-Wayland MCP server.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--socket",
        default=SOCKET_PATH,
        metavar="PATH",
        help=f"Unix socket path (default: {SOCKET_PATH})",
    )
    parser.add_argument(
        "--prompt",
        default="",
        metavar="TEXT",
        help="Text to speak before recording (default: built-in prompt)",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=15.0,
        metavar="SECONDS",
        help="Max seconds to wait for speech (default: 15)",
    )
    parser.add_argument(
        "--no-echo",
        action="store_true",
        help="Skip speaking the transcript back via TTS",
    )
    parser.add_argument(
        "--list-tools",
        action="store_true",
        help="Print the tool list from the server and exit",
    )
    args = parser.parse_args()
    sys.exit(run_test(args))


if __name__ == "__main__":
    main()
