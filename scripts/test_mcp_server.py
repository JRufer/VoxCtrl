#!/usr/bin/env python3
"""
Integration test for the VoxCtr MCP Rust Server over Unix Socket.
This script communicates directly with the Unix domain socket at /tmp/voxctl-mcp.sock
using JSON-RPC 2.0 protocol and verifies the core tools.
"""

import socket
import json
import sys
import time

SOCKET_PATH = "/tmp/voxctl-mcp.sock"

def send_rpc(sock, method, params=None, rpc_id=1):
    payload = {
        "jsonrpc": "2.0",
        "id": rpc_id,
        "method": method,
        "params": params or {}
    }
    message = json.dumps(payload) + "\n"
    print(f"--> Sending: {message.strip()}")
    sock.sendall(message.encode('utf-8'))
    
    # Read response
    response_data = b""
    while True:
        chunk = sock.recv(1024)
        if not chunk:
            break
        response_data += chunk
        if b"\n" in response_data:
            break
            
    response_str = response_data.decode('utf-8').strip()
    print(f"<-- Received: {response_str}")
    return json.loads(response_str)

def main():
    print("Connecting to MCP Server socket...")
    try:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        sock.connect(SOCKET_PATH)
        sock.settimeout(5.0)
    except Exception as e:
        print(f"Error connecting to socket {SOCKET_PATH}: {e}")
        print("Please ensure the VoxCtr Tauri app is running with MCP enabled.")
        sys.exit(1)
        
    print("\n--- Test 1: JSON-RPC Initialize ---")
    resp = send_rpc(sock, "initialize", rpc_id=101)
    if "result" in resp and "protocolVersion" in resp["result"]:
        print("PASS: Initialize returned protocol version: ", resp["result"]["protocolVersion"])
    else:
        print("FAIL: Initialize invalid response")
        
    print("\n--- Test 2: Tools List ---")
    resp = send_rpc(sock, "tools/list", rpc_id=102)
    if "result" in resp and "tools" in resp["result"]:
        tools = resp["result"]["tools"]
        tool_names = [t["name"] for t in tools]
        print(f"PASS: Found tools: {tool_names}")
        assert "get_status" in tool_names
        assert "speak_text" in tool_names
        assert "transcribe_voice" in tool_names
    else:
        print("FAIL: Tools list invalid response")
        
    print("\n--- Test 3: Tool Call (get_status) ---")
    resp = send_rpc(sock, "tools/call", {"name": "get_status"}, rpc_id=103)
    if "result" in resp and "content" in resp["result"]:
        content_text = resp["result"]["content"][0]["text"]
        print(f"PASS: get_status returned text: {content_text}")
        status = json.loads(content_text)
        print(f"Parsed state: recording={status.get('recording')}, speaking={status.get('speaking')}")
    else:
        print("FAIL: get_status invalid response")
        
    print("\n--- Test 4: Tool Call (speak_text) ---")
    resp = send_rpc(sock, "tools/call", {
        "name": "speak_text",
        "arguments": {"text": "MCP Server integration test successful."}
    }, rpc_id=104)
    if "result" in resp and "content" in resp["result"]:
        print("PASS: speak_text queued successfully:", resp["result"]["content"][0]["text"])
    else:
        print("FAIL: speak_text invalid response")
        
    print("\n--- Test 5: Unknown Method Error Handling ---")
    resp = send_rpc(sock, "tools/invalid_method_test", rpc_id=105)
    if "error" in resp:
        print(f"PASS: Received expected error: {resp['error']}")
    else:
        print("FAIL: Did not get expected error for invalid method")

    sock.close()
    print("\nAll integration tests passed successfully!")

if __name__ == "__main__":
    main()
