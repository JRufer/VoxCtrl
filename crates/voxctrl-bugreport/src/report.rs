//! The report itself: what is in it, and how it reads.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::logs::LogExcerpt;
use crate::sysinfo::SystemInfo;

/// Bumped when the shape below changes, so the relay can tell an old client's
/// report from a new one instead of guessing.
pub const SCHEMA_VERSION: u32 = 1;

/// What the user says about the bug. Everything else in a report is gathered;
/// this is the part they wrote.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UserStatement {
    /// One line, used as the issue title.
    pub summary: String,
    /// What happened, what was expected, how to reproduce it.
    pub description: String,
    /// Which part of the app, from a fixed list. Free text would be another
    /// place for something personal to end up, and a dropdown labels better.
    pub area: String,
    /// "always", "sometimes" or "once".
    pub frequency: String,
}

impl UserStatement {
    /// The text the duplicate check hashes.
    pub fn fingerprint_source(&self) -> String {
        format!("{}\n{}", self.summary, self.description)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSection {
    pub text: String,
    pub lines: usize,
    pub truncated: bool,
}

impl From<LogExcerpt> for LogSection {
    fn from(excerpt: LogExcerpt) -> Self {
        Self {
            text: excerpt.text,
            lines: excerpt.lines,
            truncated: excerpt.truncated,
        }
    }
}

/// A complete report, exactly as it is sent and exactly as it is shown to the
/// user in the preview before they send it. There is no second, fuller version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BugReport {
    pub schema: u32,
    pub created_at: DateTime<Utc>,
    /// Random per-installation identifier; see [`crate::throttle::new_install_id`].
    pub install_id: String,
    /// Hash of the user's words, so the relay can recognise a resend.
    pub fingerprint: String,
    pub statement: UserStatement,
    pub system: SystemInfo,
    /// The redacted configuration.
    pub config: Value,
    pub targets: Value,
    pub bindings: Value,
    pub log: LogSection,
}

impl BugReport {
    /// The issue title: the user's summary, trimmed to something a tracker can
    /// display, with the platform in front so a Windows-only bug is obvious in
    /// a list.
    pub fn issue_title(&self) -> String {
        let summary = collapse_whitespace(&self.statement.summary);
        let summary = truncate_chars(&summary, 120);
        let platform = match self.system.os.as_str() {
            "windows" => "Windows",
            "linux" => "Linux",
            "macos" => "macOS",
            other => other,
        };
        format!("[{platform}] {summary}")
    }

    /// The report as GitHub-flavoured Markdown — the issue body, the clipboard
    /// contents and the saved file are all this same text.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let s = &self.statement;

        out.push_str("### What happened\n\n");
        out.push_str(&neutralize_user_text(s.description.trim()));
        out.push_str("\n\n");

        out.push_str("| | |\n|---|---|\n");
        out.push_str(&format!("| Area | {} |\n", md_cell(&s.area)));
        out.push_str(&format!("| How often | {} |\n", md_cell(&s.frequency)));
        out.push_str(&format!(
            "| Reported | {} |\n",
            self.created_at.format("%Y-%m-%d %H:%M UTC")
        ));
        out.push_str(&format!("| Report ID | `{}` |\n\n", md_cell(&self.fingerprint[..12.min(self.fingerprint.len())])));

        out.push_str("### System\n\n");
        out.push_str(&self.system_table());
        out.push('\n');

        out.push_str(&details(
            "Settings (secrets, prompts, vocabulary and file paths removed)",
            &fenced("json", &pretty(&self.config)),
        ));
        out.push_str(&details(
            "Output targets and hotkeys (shape only — labels, commands and URLs removed)",
            &format!(
                "{}\n{}",
                fenced("json", &pretty(&self.targets)),
                fenced("json", &pretty(&self.bindings))
            ),
        ));

        let log_heading = if self.log.truncated {
            format!("Log — last {} lines (older lines trimmed)", self.log.lines)
        } else {
            format!("Log — {} lines", self.log.lines)
        };
        out.push_str(&details(&log_heading, &fenced("text", &self.log.text)));

        out.push_str(
            "\n---\n_Filed from VoxCtrl's Bug Report page. The reporter's words are the \
             \"What happened\" section above; everything else was gathered by the app under the \
             rules in [docs/bug_reports.md](https://github.com/JRufer/VoxCtrl/blob/master/docs/bug_reports.md)._\n",
        );
        out
    }

    fn system_table(&self) -> String {
        let s = &self.system;
        let mut rows: Vec<(&str, String)> = vec![
            ("VoxCtrl", format!("{} ({})", s.app_version, s.install_kind)),
            ("Build features", or_none(&s.build_features.join(", "))),
            (
                "GPU offload in this build",
                format!(
                    "whisper.cpp: {} · Moonshine: {}",
                    s.whisper_gpu.as_deref().unwrap_or("none (CPU)"),
                    s.moonshine_gpu.as_deref().unwrap_or("none (CPU)")
                ),
            ),
            (
                "OS",
                format!(
                    "{} — {} {}",
                    s.os,
                    s.os_name.as_deref().unwrap_or("unknown"),
                    s.os_version.as_deref().unwrap_or("")
                )
                .trim_end()
                .to_string(),
            ),
            ("Architecture", s.arch.clone()),
            (
                "CPU",
                format!(
                    "{} ({} logical cores)",
                    s.cpu_model.as_deref().unwrap_or("unknown"),
                    s.cpu_logical_cores
                        .map(|n| n.to_string())
                        .unwrap_or_else(|| "?".into())
                ),
            ),
            (
                "Memory",
                s.memory_total_mb
                    .map(|mb| format!("{} MB", mb))
                    .unwrap_or_else(|| "unknown".into()),
            ),
            ("Display adapter", or_none(&s.gpus.join(" · "))),
        ];
        if let Some(desktop) = &s.desktop {
            rows.push(("Desktop", desktop.clone()));
        }
        if let Some(session) = &s.session_type {
            rows.push(("Session", session.clone()));
        }
        if let Some(language) = &s.language {
            rows.push(("Language", language.clone()));
        }

        let mut table = String::from("| | |\n|---|---|\n");
        for (label, value) in rows {
            table.push_str(&format!("| {label} | {} |\n", md_cell(&value)));
        }
        if !s.collection_notes.is_empty() {
            table.push_str(&format!(
                "\n_Not collected: {}._\n",
                s.collection_notes.join("; ")
            ));
        }
        table
    }
}

/// Defuse the two things a bug description can do to an issue tracker that
/// have nothing to do with reporting a bug.
///
/// `@name` in an issue body notifies that person, and `#123` cross-links
/// another issue and posts a backlink into it. A description made of five
/// hundred of either is a spam delivery mechanism wearing a bug report as a
/// disguise, and it costs the recipients, not the reporter. A zero-width space
/// after the sigil stops both: it reads identically, copies identically, and
/// GitHub no longer treats it as a reference.
///
/// Tag-looking `<` is escaped for a duller reason — a stray `</details>` in a
/// description would close the report's own collapsible sections early and
/// hide the rest of it. `x < y` is left alone.
///
/// This happens at render time, so the preview the reporter reads is exactly
/// the text that gets filed.
fn neutralize_user_text(text: &str) -> String {
    const ZERO_WIDTH_SPACE: char = '\u{200b}';
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());

    for (i, c) in chars.iter().enumerate() {
        let next = chars.get(i + 1).copied();
        match c {
            '@' if next.is_some_and(|n| n.is_alphanumeric()) => {
                out.push('@');
                out.push(ZERO_WIDTH_SPACE);
            }
            '#' if next.is_some_and(|n| n.is_ascii_digit()) => {
                out.push('#');
                out.push(ZERO_WIDTH_SPACE);
            }
            '<' if next.is_some_and(|n| n.is_ascii_alphabetic() || n == '/') => {
                out.push_str("&lt;");
            }
            other => out.push(*other),
        }
    }
    out
}

fn or_none(text: &str) -> String {
    if text.trim().is_empty() {
        "none reported".into()
    } else {
        text.to_string()
    }
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

fn details(summary: &str, body: &str) -> String {
    format!("<details>\n<summary>{summary}</summary>\n\n{body}\n</details>\n\n")
}

/// Wrap in a fence long enough to survive whatever the body contains — a log
/// line holding three backticks would otherwise end the block early and spill
/// the rest of the report into the page as markup.
fn fenced(language: &str, body: &str) -> String {
    let longest_run = body
        .split(|c| c != '`')
        .map(|run| run.len())
        .max()
        .unwrap_or(0);
    let fence = "`".repeat(longest_run.max(2) + 1);
    format!("{fence}{language}\n{body}\n{fence}\n")
}

/// A pipe inside a table cell ends the cell, so it has to be escaped; a newline
/// ends the row entirely, so it becomes a space.
fn md_cell(text: &str) -> String {
    collapse_whitespace(text).replace('|', "\\|")
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let kept: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> BugReport {
        BugReport {
            schema: SCHEMA_VERSION,
            created_at: DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            install_id: "0123456789abcdef0123456789abcdef".into(),
            fingerprint: "abcdef0123456789".into(),
            statement: UserStatement {
                summary: "Recording stops after one second".into(),
                description: "I press the hotkey and it records for about a second, then stops."
                    .into(),
                area: "Hotkeys".into(),
                frequency: "always".into(),
            },
            system: SystemInfo {
                app_version: "0.5.1".into(),
                install_kind: "installed".into(),
                os: "windows".into(),
                os_name: Some("Microsoft Windows 11 Pro".into()),
                arch: "x86_64".into(),
                ..SystemInfo::default()
            },
            config: json!({"engine": {"backend": "whisper-cpp"}}),
            targets: json!([]),
            bindings: json!([]),
            log: LogSection {
                text: "2026-01-01 [WARN] voxctrl: something".into(),
                lines: 1,
                truncated: false,
            },
        }
    }

    #[test]
    fn the_title_leads_with_the_platform() {
        assert_eq!(
            report().issue_title(),
            "[Windows] Recording stops after one second"
        );
    }

    #[test]
    fn a_summary_cannot_smuggle_newlines_into_the_title() {
        let mut r = report();
        r.statement.summary = "line one\nline two".into();
        assert_eq!(r.issue_title(), "[Windows] line one line two");
    }

    #[test]
    fn a_very_long_summary_is_trimmed() {
        let mut r = report();
        r.statement.summary = "x".repeat(300);
        assert!(r.issue_title().chars().count() <= 132);
        assert!(r.issue_title().ends_with('…'));
    }

    #[test]
    fn the_markdown_leads_with_what_the_user_wrote() {
        let markdown = report().to_markdown();
        assert!(markdown.starts_with("### What happened"));
        assert!(markdown.contains("records for about a second"));
    }

    #[test]
    fn a_log_full_of_backticks_cannot_break_out_of_its_code_block() {
        // A log line holding a fence would otherwise close the block early and
        // the rest of the report would render as markup — or worse, as a link.
        let mut r = report();
        r.log.text = "user typed ``` and then ```` more".into();
        let markdown = r.to_markdown();
        let fence = "`".repeat(5);
        assert!(
            markdown.contains(&format!("{fence}text")),
            "expected a fence longer than the content: {markdown}"
        );
    }

    #[test]
    fn a_pipe_in_a_field_does_not_break_the_table() {
        let mut r = report();
        r.statement.area = "Audio | Devices".into();
        assert!(r.to_markdown().contains("Audio \\| Devices"));
    }

    #[test]
    fn a_description_cannot_be_used_to_notify_five_hundred_people() {
        let mut r = report();
        r.statement.description = "cc @octocat @torvalds, same as #42".into();
        let markdown = r.to_markdown();
        assert!(!markdown.contains("@octocat"), "the mention survived: {markdown}");
        assert!(!markdown.contains("#42"), "the cross-reference survived: {markdown}");
        // It still reads as what they wrote.
        assert!(markdown.contains("cc @\u{200b}octocat"));
    }

    #[test]
    fn an_email_address_in_a_description_is_left_readable() {
        // The `@` rule must not mangle the thing a reporter is most likely to
        // type on purpose.
        let mut r = report();
        r.statement.description = "reply to me at jane at example.com if needed".into();
        assert!(r.to_markdown().contains("jane at example.com"));
    }

    #[test]
    fn a_stray_closing_tag_cannot_collapse_the_report() {
        let mut r = report();
        r.statement.description = "it printed </details> and then stopped".into();
        let markdown = r.to_markdown();
        assert!(markdown.contains("&lt;/details>"));
        // The report's own sections are still intact below the description.
        assert!(markdown.contains("<summary>Settings"));
    }

    #[test]
    fn arithmetic_in_a_description_is_not_escaped() {
        let mut r = report();
        r.statement.description = "it only happens when gain < 1 and threshold > 0".into();
        assert!(r.to_markdown().contains("gain < 1"));
    }

    #[test]
    fn the_body_says_where_the_rules_are_written_down() {
        // Someone reading the issue should be able to check what was collected
        // without taking the app's word for it.
        assert!(report().to_markdown().contains("docs/bug_reports.md"));
    }
}
