//! Watches a named pipe (FIFO) for lines of text and speaks each one as it
//! arrives. Used to let external tools/scripts trigger TTS without going
//! through the MCP server.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::engine::TtsEngineHandle;

/// Speak every line written to `fifo_path` through whichever TTS worker is
/// current when the line arrives.
///
/// The slot is looked up per line rather than a handle being captured once:
/// the worker is replaced whenever TTS is turned off and on again (and once at
/// startup), and a responder bound to the handle it was spawned with would keep
/// speaking into a worker that had already shut down — silently.
pub async fn run_fifo_responder(fifo_path: String, tts: Arc<Mutex<Option<TtsEngineHandle>>>) {
    use tokio::io::{AsyncBufReadExt, BufReader};

    info!("FIFO responder watching {fifo_path}");
    loop {
        while !std::path::Path::new(&fifo_path).exists() {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        match tokio::fs::File::open(&fifo_path).await {
            Ok(file) => {
                let mut lines = BufReader::new(file).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line.trim().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    match tts.lock().await.as_ref() {
                        Some(tts) => tts.speak(line),
                        None => debug!("TTS is disabled; dropping line from {fifo_path}"),
                    }
                }
            }
            Err(e) => {
                warn!("FIFO open error {fifo_path}: {e}; retrying");
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
