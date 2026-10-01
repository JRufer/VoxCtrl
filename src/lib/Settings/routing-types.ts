export interface TargetProcessingConfig {
  remove_fillers?: boolean;
  spoken_punctuation?: boolean;
  auto_format_lists?: boolean;
  code_mode?: boolean;
}

export interface OutputTarget {
  id: string;
  label: string;
  delivery: string;
  command?: string;
  pipe_path?: string;
  socket_host?: string;
  socket_port?: number;
  socket_unix?: string;
  file_path?: string;
  file_prefix: string;
  file_timestamp: boolean;
  file_timestamp_format: string;
  file_mode?: string;
  dbus_signal?: string;
  http_url?: string;
  http_method: string;
  http_headers?: Record<string, string>;
  http_json_template?: Record<string, any>;
  webhook_url?: string;
  webhook_secret?: string;
  webhook_json_template?: Record<string, any>;
  mcp_path?: string;
  mcp_tool?: string;
  mcp_args?: Record<string, any>;
  chat_url?: string;
  chat_model?: string;
  chat_api_key?: string;
  chat_system_prompt?: string;
  chat_max_history: number;
  chat_timeout_secs: number;
  chat_reply_mode: string;
  chat_reset_phrase?: string;
  strip_newlines: boolean;
  processing?: TargetProcessingConfig;
  response_pipe?: string;
}

export interface HotkeyBinding {
  id: string;
  keys: string[];
  gesture: string;
  target_id: string;
  target_ids?: string[];
  tap_ms: number;
  hold_threshold_ms: number;
  label: string;
  disabled: boolean;
  openai_enabled?: boolean;
  openai_model?: string;
  openai_mode?: string;
  openai_prompt?: string;
  openai_system_prompt?: string;
  s1_mini_enabled?: boolean;
}

/** A fresh Output Command with the editor's defaults, under a random id. */
export function newOutputTarget(): OutputTarget {
  return {
    id: "new_target_" + Math.random().toString(36).substring(2, 6),
    label: "New Target",
    delivery: "inject",
    file_prefix: "- ",
    file_timestamp: true,
    file_timestamp_format: "%Y-%m-%dT%H:%M:%SZ",
    file_mode: "append",
    http_method: "POST",
    http_json_template: { text: "{text}" },
    webhook_json_template: { text: "{text}" },
    mcp_tool: "speak_text",
    mcp_args: { text: "{text}" },
    chat_max_history: 20,
    chat_timeout_secs: 120,
    chat_reply_mode: "speak",
    strip_newlines: false,
    processing: {},
  };
}
