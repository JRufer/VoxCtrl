use crate::models::{DeliveryType, OutputTarget};
use crate::loader::{load_targets, save_targets};
use crate::targets::build_target;

#[test]
fn test_mcp_config_roundtrip() {
    let temp_dir = std::env::temp_dir().join(format!("voxctr_test_{}", chrono::Utc::now().timestamp_millis()));
    std::fs::create_dir_all(&temp_dir).unwrap();

    let target = OutputTarget {
        id: "mcp_test".into(),
        label: "Test MCP".into(),
        delivery: DeliveryType::Mcp,
        command: None,
        pipe_path: None,
        socket_host: None,
        socket_port: None,
        socket_unix: None,
        file_path: None,
        file_prefix: "".into(),
        file_timestamp: false,
        dbus_signal: None,
        http_url: None,
        http_method: "POST".into(),
        http_headers: None,
        http_json_template: None,
        webhook_url: None,
        webhook_secret: None,
        webhook_json_template: None,
        mcp_path: Some("/tmp/custom-mcp.sock".into()),
        mcp_tool: Some("speak_text".into()),
        mcp_args: Some(serde_json::json!({ "message": "{TEXT}" })),
        send_on_release: true,
        append_newline: false,
        initial_prompt: None,
        processing: Default::default(),
        response_pipe: None,
        tts_engine: "piper".into(),
        tts_voice: None,
    };

    save_targets(&[target.clone()], &temp_dir).unwrap();
    let loaded = load_targets(&temp_dir).unwrap();

    assert_eq!(loaded.len(), 1);
    let loaded_target = &loaded[0];
    assert_eq!(loaded_target.id, "mcp_test");
    assert_eq!(loaded_target.delivery, DeliveryType::Mcp);
    assert_eq!(loaded_target.mcp_path.as_deref(), Some("/tmp/custom-mcp.sock"));
    assert_eq!(loaded_target.mcp_tool.as_deref(), Some("speak_text"));
    assert_eq!(
        loaded_target.mcp_args.as_ref().unwrap(),
        &serde_json::json!({ "message": "{TEXT}" })
    );

    let _ = std::fs::remove_dir_all(&temp_dir);
}

#[tokio::test]
async fn test_mcp_delivery_handshake() {
    use tokio::net::UnixListener;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let socket_path = format!("/tmp/voxctr-mcp-test-{}.sock", chrono::Utc::now().timestamp_millis());
    let socket_path_clone = socket_path.clone();

    // 1. Start a mock MCP server that implements the handshake protocol
    let server = tokio::spawn(async move {
        let listener = UnixListener::bind(&socket_path_clone).unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let (reader, mut writer) = tokio::io::split(stream);
        let mut lines = BufReader::new(reader).lines();

        // Receive initialize
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("initialize"));
        let init_res = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": "2024-11-05",
                "serverInfo": { "name": "mock-mcp", "version": "1.0" },
                "capabilities": {}
            }
        });
        writer.write_all((serde_json::to_string(&init_res).unwrap() + "\n").as_bytes()).await.unwrap();
        writer.flush().await.unwrap();

        // Receive notifications/initialized
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("notifications/initialized"));

        // Receive tools/call
        let line = lines.next_line().await.unwrap().unwrap();
        assert!(line.contains("tools/call"));
        assert!(line.contains("speak_text"));
        assert!(line.contains("Hello World"));

        let tool_res = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "content": [
                    { "type": "text", "text": "spoken" }
                ]
            }
        });
        writer.write_all((serde_json::to_string(&tool_res).unwrap() + "\n").as_bytes()).await.unwrap();
        writer.flush().await.unwrap();
    });

    // Give the server a small moment to bind
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 2. Deliver the text via McpTarget
    let config = OutputTarget {
        id: "mcp_test".into(),
        label: "Test MCP".into(),
        delivery: DeliveryType::Mcp,
        command: None,
        pipe_path: None,
        socket_host: None,
        socket_port: None,
        socket_unix: None,
        file_path: None,
        file_prefix: "".into(),
        file_timestamp: false,
        dbus_signal: None,
        http_url: None,
        http_method: "POST".into(),
        http_headers: None,
        http_json_template: None,
        webhook_url: None,
        webhook_secret: None,
        webhook_json_template: None,
        mcp_path: Some(socket_path.clone()),
        mcp_tool: Some("speak_text".into()),
        mcp_args: Some(serde_json::json!({ "text": "{TEXT}" })),
        send_on_release: true,
        append_newline: false,
        initial_prompt: None,
        processing: Default::default(),
        response_pipe: None,
        tts_engine: "piper".into(),
        tts_voice: None,
    };

    let target = build_target(config);
    let result = target.deliver("Hello World").await;

    assert!(result.success);
    assert!(result.error.is_none());

    server.await.unwrap();
    let _ = std::fs::remove_file(&socket_path);
}
