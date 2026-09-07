//! The log excerpt a report carries.
//!
//! VoxCtrl keeps one log file, `startup_errors.log`, and it is already written
//! with this moment in mind: `src-tauri/src/startup_log.rs` drops any line
//! whose message mentions transcription, speech or payloads before it is ever
//! written, so dictated text cannot reach the file and therefore cannot reach a
//! report. What is left is startup diagnostics, warnings and errors.
//!
//! This module does three further things to it: takes only the tail, scrubs
//! home directories and account names out of every line, and caps the size so
//! a report cannot become a megabyte of someone's disk.

use std::path::{Path, PathBuf};

use crate::scrub::Scrubber;

/// The most recent lines are the ones that explain the bug the user is
/// reporting; older ones are from sessions they have forgotten about.
pub const MAX_LINES: usize = 400;

/// A ceiling in bytes, applied after the line limit. A single log line can be a
/// wrapped panic message thousands of characters long, so the line count alone
/// is not a bound on size.
pub const MAX_BYTES: usize = 64 * 1024;

/// Where the running app writes its log.
pub fn log_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("voxctrl")
        .join("startup_errors.log")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogExcerpt {
    pub text: String,
    /// How many lines the excerpt holds.
    pub lines: usize,
    /// True when the file was longer than the excerpt.
    pub truncated: bool,
}

impl LogExcerpt {
    fn empty(text: &str) -> Self {
        Self {
            text: text.to_string(),
            lines: 0,
            truncated: false,
        }
    }
}

/// Read the tail of VoxCtrl's log, scrubbed and capped.
pub fn collect(scrubber: &Scrubber) -> LogExcerpt {
    read_from(&log_path(), scrubber)
}

pub fn read_from(path: &Path, scrubber: &Scrubber) -> LogExcerpt {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return LogExcerpt::empty("<no log file; VoxCtrl has not written one on this machine>");
    };
    let excerpt = tail(&raw, MAX_LINES, MAX_BYTES);
    if excerpt.text.trim().is_empty() {
        return LogExcerpt::empty("<the log file is empty>");
    }
    LogExcerpt {
        text: scrubber.scrub(&excerpt.text),
        ..excerpt
    }
}

/// The last `max_lines` lines, then the last `max_bytes` bytes of those.
fn tail(text: &str, max_lines: usize, max_bytes: usize) -> LogExcerpt {
    let all: Vec<&str> = text.lines().collect();
    let mut truncated = all.len() > max_lines;
    let start = all.len().saturating_sub(max_lines);
    let mut kept = all[start..].join("\n");

    if kept.len() > max_bytes {
        truncated = true;
        // Cut on a character boundary, then forward to the next line break so
        // the excerpt does not open mid-line.
        let mut cut = kept.len() - max_bytes;
        while cut < kept.len() && !kept.is_char_boundary(cut) {
            cut += 1;
        }
        let from = kept[cut..].find('\n').map(|n| cut + n + 1).unwrap_or(cut);
        kept = kept[from..].to_string();
    }

    LogExcerpt {
        lines: kept.lines().count(),
        text: kept,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        Scrubber::new(Some("/home/jane".into()), Some("jane".into()))
    }

    #[test]
    fn only_the_tail_is_kept() {
        let text: String = (1..=1000).map(|n| format!("line {n}\n")).collect();
        let excerpt = tail(&text, 400, MAX_BYTES);
        assert_eq!(excerpt.lines, 400);
        assert!(excerpt.truncated);
        assert!(excerpt.text.starts_with("line 601\n"));
        assert!(excerpt.text.ends_with("line 1000"));
    }

    #[test]
    fn a_short_log_is_not_reported_as_truncated() {
        let excerpt = tail("one\ntwo\nthree", 400, MAX_BYTES);
        assert_eq!(excerpt.lines, 3);
        assert!(!excerpt.truncated);
    }

    #[test]
    fn a_byte_cap_applies_on_top_of_the_line_cap() {
        // One enormous line — a wrapped panic — beats the line limit, so the
        // byte limit has to be what actually bounds the report.
        let text = format!("first\n{}\nlast\n", "x".repeat(200_000));
        let excerpt = tail(&text, 400, 1024);
        assert!(excerpt.text.len() <= 1024, "got {} bytes", excerpt.text.len());
        assert!(excerpt.truncated);
    }

    #[test]
    fn the_excerpt_starts_at_a_line_break_not_mid_line() {
        let text: String = (1..=500).map(|n| format!("line {n} padding padding\n")).collect();
        let excerpt = tail(&text, 400, 500);
        assert!(
            excerpt.text.starts_with("line "),
            "excerpt opened mid-line: {:?}",
            &excerpt.text[..40.min(excerpt.text.len())]
        );
    }

    #[test]
    fn paths_and_account_names_are_scrubbed_out_of_log_lines() {
        // The log is full of paths, and none of the per-field rules in
        // `redact` apply to a free-form log line — this is the only thing
        // standing between a report and the account name in every path.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("startup_errors.log");
        std::fs::write(
            &path,
            "2026-01-01T00:00:00Z [ERROR] voxctrl: could not open /home/jane/.config/voxctrl/config.json\n",
        )
        .unwrap();

        let excerpt = read_from(&path, &scrubber());
        assert!(excerpt.text.contains("~/.config/voxctrl/config.json"));
        assert!(!excerpt.text.contains("jane"));
    }

    #[test]
    fn a_missing_log_file_says_so_rather_than_failing() {
        let excerpt = read_from(Path::new("/nonexistent/voxctrl.log"), &scrubber());
        assert!(excerpt.text.contains("no log file"));
        assert_eq!(excerpt.lines, 0);
    }
}
