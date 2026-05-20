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
  dbus_signal?: string;
  http_url?: string;
  http_method: string;
  http_headers?: Record<string, string>;
  webhook_url?: string;
  webhook_secret?: string;
  send_on_release: boolean;
  append_newline: boolean;
  initial_prompt?: string;
  response_pipe?: string;
  tts_engine: string;
  tts_voice?: string;
}

export interface HotkeyBinding {
  id: string;
  keys: string[];
  gesture: string;
  target_id: string;
  tap_ms: number;
  hold_threshold_ms: number;
  label: string;
  disabled: boolean;
}
