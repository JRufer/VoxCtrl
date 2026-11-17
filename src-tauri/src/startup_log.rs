use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

pub static STARTUP_COMPLETE: AtomicBool = AtomicBool::new(false);

struct MessageVisitor {
    message: String,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        }
    }
}

pub struct StartupErrorLayer {
    file: Mutex<File>,
}

/// Past this size the log is cut back to its last [`KEEP_BYTES`] when the app
/// starts. It is opened for appending and was never trimmed, so on a machine
/// that logs a warning now and then it grew for as long as VoxCtrl was
/// installed. Bug reports only ever read its last 64 KB.
pub const MAX_LOG_BYTES: u64 = 1024 * 1024;
pub const KEEP_BYTES: usize = 256 * 1024;

/// Cut the log at `path` back to its last `keep` bytes, starting at a line,
/// if it is larger than `max`. Errors are ignored: a log that cannot be
/// trimmed is still better appended to than not opened.
pub fn trim_log(path: &std::path::Path, max: u64, keep: usize) {
    let Ok(meta) = std::fs::metadata(path) else { return };
    if meta.len() <= max {
        return;
    }
    let Ok(bytes) = std::fs::read(path) else { return };
    let start = bytes.len().saturating_sub(keep);
    // Begin after the first line break inside the kept part, so no line is cut in two.
    let start = bytes[start..].iter().position(|&b| b == b'\n').map_or(start, |i| start + i + 1);
    let partial = path.with_extension("log.part");
    if std::fs::write(&partial, &bytes[start..]).is_ok() {
        let _ = std::fs::rename(&partial, path);
    }
}

impl StartupErrorLayer {
    pub fn new(path: std::path::PathBuf) -> std::io::Result<Self> {
        trim_log(&path, MAX_LOG_BYTES, KEEP_BYTES);
        let file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
        Ok(Self { file: Mutex::new(file) })
    }
}

impl<S: Subscriber> Layer<S> for StartupErrorLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let metadata = event.metadata();
        let level = metadata.level();

        let mut visitor = MessageVisitor { message: String::new() };
        event.record(&mut visitor);

        let msg = visitor.message;

        // Apply strict filtering rules to prevent any user text from leaking into the log.
        let lower_msg = msg.to_lowercase();
        if lower_msg.contains("received transcription")
            || lower_msg.contains("delivered target_id")
            || lower_msg.contains("transcribe")
            || lower_msg.contains("transcription")
            || lower_msg.contains("speaking")
            || lower_msg.contains("speak")
            || lower_msg.contains("openai")
            || lower_msg.contains("payload")
            || lower_msg.contains("status-tick")
        {
            return;
        }

        let is_error_or_warn = *level == tracing::Level::ERROR || *level == tracing::Level::WARN;
        let is_startup = !STARTUP_COMPLETE.load(Ordering::SeqCst);

        if is_error_or_warn || (is_startup && *level == tracing::Level::INFO) {
            let timestamp = chrono::Utc::now().to_rfc3339();
            let log_line = format!("{} [{}] {}: {}\n", timestamp, level, metadata.target(), msg);
            if let Ok(mut file) = self.file.lock() {
                let _ = file.write_all(log_line.as_bytes());
                let _ = file.flush();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_log_is_cut_back_to_whole_lines_and_a_small_one_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("startup_errors.log");
        let lines: String = (0..1000).map(|i| format!("line {i:04}\n")).collect();
        std::fs::write(&path, &lines).unwrap();

        trim_log(&path, lines.len() as u64, 100);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), lines, "not over the limit: untouched");

        trim_log(&path, 1000, 100);
        let kept = std::fs::read_to_string(&path).unwrap();
        assert!(kept.len() <= 100, "{}", kept.len());
        assert!(kept.starts_with("line "), "starts at a whole line: {kept:?}");
        assert!(kept.ends_with("line 0999\n"));
        assert!(!dir.path().join("startup_errors.log.part").exists());
    }
}
