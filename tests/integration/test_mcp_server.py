import socket
import json
import pytest
import os

SOCKET_PATH = "/tmp/voxctl-mcp.sock"

def send_rpc(sock, method, params=None, rpc_id=1):
    payload = {
        "jsonrpc": "2.0",
        "id": rpc_id,
        "method": method,
        "params": params or {}
    }
    message = json.dumps(payload) + "\n"
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
    return json.loads(response_str)

@pytest.mark.skipif(not os.path.exists(SOCKET_PATH), reason="MCP Socket not running")
def test_mcp_handshake_and_tools():
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.connect(SOCKET_PATH)
    sock.settimeout(5.0)
    
    try:
        # Test 1: Initialize
        resp = send_rpc(sock, "initialize", rpc_id=101)
        assert "result" in resp
        assert "protocolVersion" in resp["result"]
        
        # Test 2: Tools List
        resp = send_rpc(sock, "tools/list", rpc_id=102)
        assert "result" in resp
        assert "tools" in resp["result"]
        tools = resp["result"]["tools"]
        tool_names = [t["name"] for t in tools]
        assert "get_status" in tool_names
        assert "speak_text" in tool_names
        assert "transcribe_voice" in tool_names
        
        # Test 3: Tool Call (get_status)
        resp = send_rpc(sock, "tools/call", {"name": "get_status"}, rpc_id=103)
        assert "result" in resp
        assert "content" in resp["result"]
        content_text = resp["result"]["content"][0]["text"]
        status = json.loads(content_text)
        assert "recording" in status
        assert "speaking" in status
        
        # Test 4: Tool Call (speak_text)
        resp = send_rpc(sock, "tools/call", {
            "name": "speak_text",
            "arguments": {"text": "Integration test successful."}
        }, rpc_id=104)
        assert "result" in resp
        assert "content" in resp["result"]
        
        # Test 5: Unknown Method Error
        resp = send_rpc(sock, "tools/invalid_method_test", rpc_id=105)
        assert "error" in resp
        
    finally:
        sock.close()
