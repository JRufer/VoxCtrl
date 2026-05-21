#!/usr/bin/env python3
"""
Simple script to send a speak_text tool call to the VoxCtr Rust MCP server.
Usage:
    python scripts/speak_mcp.py "Hello, this is a custom sentence spoken back to you!"
"""

import socket
import json
import sys

SOCKET_PATH = "/tmp/voxctl-mcp.sock"

def speak(text):
    print(f"Connecting to MCP Server at {SOCKET_PATH}...")
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(SOCKET_PATH)
        sock.settimeout(5.0)
    except Exception as e:
        print(f"Error connecting: {e}")
        print("Please ensure the VoxCtr Tauri app is running with MCP enabled.")
        sys.exit(1)

    # Step 1: Initialize MCP Session
    init_payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    }
    sock.sendall((json.dumps(init_payload) + "\n").encode('utf-8'))
    
    # Read response
    init_resp = b""
    while True:
        chunk = sock.recv(1024)
        if not chunk:
            break
        init_resp += chunk
        if b"\n" in init_resp:
            break
    print("--> MCP Handshake successful.")

    # Step 2: Call speak_text
    speak_payload = {
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": "speak_text",
            "arguments": {"text": text}
        }
    }
    print(f"--> Sending speak request: \"{text}\"")
    sock.sendall((json.dumps(speak_payload) + "\n").encode('utf-8'))
    
    # Read response
    speak_resp = b""
    while True:
        chunk = sock.recv(1024)
        if not chunk:
            break
        speak_resp += chunk
        if b"\n" in speak_resp:
            break
            
    resp_obj = json.loads(speak_resp.decode('utf-8').strip())
    if "result" in resp_obj and "content" in resp_obj["result"]:
        print(f"✔ Success: TTS queued successfully (Response: {resp_obj['result']['content'][0]['text']})")
    elif "error" in resp_obj:
        print(f"✖ Error: {resp_obj['error']}")
    else:
        print(f"✖ Unknown response: {resp_obj}")

    sock.close()

if __name__ == "__main__":
    sentence = "Hello! I am the updated Rust M C P server, now fully aligned with the official protocol. Your Text-to-Speech engine is operational."
    if len(sys.argv) > 1:
        sentence = " ".join(sys.argv[1:])
    speak(sentence)
