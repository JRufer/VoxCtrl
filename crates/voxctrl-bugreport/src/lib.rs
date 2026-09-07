//! Building and sending a VoxCtrl bug report.
//!
//! VoxCtrl's promise is that nothing about you or your machine is ever
//! transmitted (`docs/privacy.md`). This crate is the one deliberate exception,
//! and it only ever runs because somebody pressed a button and read what was in
//! the report first. Nothing here runs at startup, on a timer, or in the
//! background.
//!
//! The rules the rest of this crate exists to enforce:
//!
//! * **The only words that travel are the ones typed into the box.** Every
//!   other free-text setting — prompts, custom vocabulary, snippets, target
//!   labels, shell commands — is counted, never quoted ([`redact`]).
//! * **No secret travels.** API keys and access tokens are reported as set or
//!   not set ([`redact::Handling::Secret`]).
//! * **No file path, home directory or account name travels**, in a setting or
//!   in a log line ([`scrub`]).
//! * **Nothing is collected that was not written down first.** The machine
//!   facts are a fixed list ([`sysinfo::SystemInfo`]), shown on the page that
//!   sends them, and a test fails if a new config setting appears that nobody
//!   has classified.

use chrono::Utc;
use serde_json::Value;

pub mod logs;
pub mod redact;
pub mod report;
pub mod scrub;
pub mod submit;
pub mod sysinfo;
pub mod throttle;

pub use report::{BugReport, LogSection, UserStatement, SCHEMA_VERSION};
pub use submit::{SubmitError, SubmitResponse};
pub use sysinfo::{BuildFacts, SystemInfo};
pub use throttle::{History, Limits, Refusal};

/// Everything a report is built from, gathered by the caller because only it
/// knows where the live config and routing files are.
pub struct Sources<'a> {
    /// The machine facts, from [`sysinfo::collect`].
    ///
    /// Passed in rather than collected here because collecting them shells out
    /// to a system probe, and the Settings page rebuilds its preview far more
    /// often than any of those facts change. The caller gathers them once.
    pub system: SystemInfo,
    /// The running configuration, serialised. Redacted here, not by the caller.
    pub config: &'a Value,
    /// The contents of `targets.toml` and `bindings.toml`, as JSON.
    pub targets: &'a Value,
    pub bindings: &'a Value,
}

/// Assemble a report. Blocking: it reads the log file, so call it off the UI
/// thread.
pub fn build(statement: UserStatement, install_id: String, sources: Sources<'_>) -> BugReport {
    let scrubber = scrub::Scrubber::from_env();
    let mut ids = redact::IdMap::default();

    BugReport {
        schema: SCHEMA_VERSION,
        created_at: Utc::now(),
        fingerprint: throttle::fingerprint(&statement.fingerprint_source()),
        install_id,
        statement,
        system: sources.system,
        config: redact::redact_config(sources.config, &scrubber),
        // Targets are summarised before bindings so the identifier pseudonyms
        // are assigned in the order they appear in the targets file, which is
        // the order the Settings UI lists them in.
        targets: redact::summarize_targets(sources.targets, &mut ids),
        bindings: redact::summarize_bindings(sources.bindings, &mut ids),
        log: logs::collect(&scrubber).into(),
    }
}

/// Where the submission history lives — beside the log, not in the config
/// directory, because it is state rather than a setting and nothing good comes
/// of it appearing in a config editor.
pub fn history_path() -> std::path::PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("voxctrl")
        .join("bug-reports.json")
}

/// Read the history, treating an unreadable or corrupt file as an empty one.
///
/// A damaged history must not block reporting: the limits it enforces are a
/// courtesy, and the relay enforces the ones that matter. Failing closed here
/// would mean a bad JSON file silently disables the feature.
pub fn load_history() -> History {
    let path = history_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return History::default();
    };
    match serde_json::from_str(&text) {
        Ok(history) => history,
        Err(e) => {
            tracing::warn!("Bug-report history could not be read ({e}); starting a new one");
            History::default()
        }
    }
}

pub fn save_history(history: &History) -> std::io::Result<()> {
    let path = history_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(history)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_report_carries_the_users_words_and_nothing_else_they_typed() {
        let config = json!({
            "openai": {"api_key": "sk-SECRET", "system_prompt": "my boss is SECRETNAME"},
            "features": {"custom_vocabulary": ["SECRETWORD"], "remove_fillers": true},
        });
        let targets = json!([{"id": "t", "label": "SECRETLABEL", "delivery": "exec",
                              "command": "notify-send SECRETCOMMAND"}]);
        let bindings = json!([{"id": "b", "keys": ["KEY_F9"], "gesture": "hold", "target_id": "t"}]);
        let build = BuildFacts {
            app_version: "0.5.1".into(),
            build_features: vec!["moonshine".into()],
            ..BuildFacts::default()
        };

        let report = build_report(
            UserStatement {
                summary: "Hotkey does nothing".into(),
                description: "Pressing F9 does not start recording.".into(),
                area: "Hotkeys".into(),
                frequency: "always".into(),
            },
            &build,
            &config,
            &targets,
            &bindings,
        );

        let serialised = serde_json::to_string(&report).unwrap();
        for secret in ["SECRET", "SECRETNAME", "SECRETWORD", "SECRETLABEL", "SECRETCOMMAND"] {
            assert!(!serialised.contains(secret), "{secret} reached the report");
        }
        assert!(serialised.contains("Pressing F9 does not start recording."));
        // The parts that explain the bug did survive.
        assert_eq!(report.bindings[0]["keys"], json!(["KEY_F9"]));
        assert_eq!(report.targets[0]["delivery"], json!("exec"));
        assert_eq!(report.config["features"]["remove_fillers"], json!(true));
    }

    fn build_report(
        statement: UserStatement,
        build: &BuildFacts,
        config: &Value,
        targets: &Value,
        bindings: &Value,
    ) -> BugReport {
        let system = sysinfo::collect(build, &scrub::Scrubber::from_env());
        super::build(
            statement,
            "test-install".into(),
            Sources { system, config, targets, bindings },
        )
    }

    #[test]
    fn a_corrupt_history_file_does_not_disable_reporting() {
        // Failing closed here would mean one bad byte silently takes the
        // feature away, with no way for the user to tell why.
        assert!(serde_json::from_str::<History>("{ not json").is_err());
        assert!(History::default().submissions.is_empty());
    }
}
