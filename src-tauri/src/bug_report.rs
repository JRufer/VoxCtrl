//! The Tauri commands behind Settings → Bug Report.
//!
//! Everything the user sees on that page comes from here, and every route out
//! of it — send, save, GitHub, email — is built from the same single report
//! object, so the preview they read is the report that goes.
//!
//! See `crates/voxctrl-bugreport` for what a report may contain and why, and
//! `docs/bug_reports.md` for the version of that written for a reader.

use std::sync::{Arc, Mutex, OnceLock};

use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tauri::State;
use voxctrl_bugreport::{
    submit, BugReport, BuildFacts, Limits, Sources, SystemInfo, UserStatement,
};

use crate::state::AppState;

/// Cargo features this binary was built with.
///
/// The whole reason the Bug Report page exists in 0.5.1 is that the Windows CPU
/// and GPU builds are the same source with different features, and a report
/// that does not say which one is running cannot be acted on.
fn build_features() -> Vec<String> {
    let mut features = Vec::new();
    if cfg!(feature = "moonshine") {
        features.push("moonshine".to_string());
    }
    if cfg!(feature = "moonshine-cuda") {
        features.push("moonshine-cuda".to_string());
    }
    if cfg!(feature = "moonshine-webgpu") {
        features.push("moonshine-webgpu".to_string());
    }
    if cfg!(feature = "moonshine-coreml") {
        features.push("moonshine-coreml".to_string());
    }
    if cfg!(feature = "inflect-micro") {
        features.push("inflect-micro".to_string());
    }
    if cfg!(feature = "cuda") {
        features.push("cuda".to_string());
    }
    if cfg!(feature = "vulkan") {
        features.push("vulkan".to_string());
    }
    if features.is_empty() {
        features.push("none (base build)".to_string());
    }
    features
}

fn build_facts() -> BuildFacts {
    BuildFacts {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        build_features: build_features(),
        whisper_gpu: voxctrl_inference::whisper_gpu_backend().map(str::to_string),
        moonshine_gpu: voxctrl_inference::moonshine_gpu_backend().map(str::to_string),
    }
}

/// The machine facts, gathered once per run.
///
/// Collecting them shells out to a system probe — PowerShell on Windows, which
/// is not quick — and the page rebuilds the preview on every keystroke pause.
/// None of what it reads changes while the app is running.
fn cached_system_info() -> SystemInfo {
    static CACHE: OnceLock<Mutex<Option<SystemInfo>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));

    // Held across the collection deliberately: two windows asking at once
    // should wait for one probe, not run two.
    let mut guard = match cache.lock() {
        Ok(guard) => guard,
        // A panic inside the probe poisoned the lock. The facts are still
        // whatever the last successful run put there, and a bug report that
        // cannot be filed because report-gathering panicked once is the worst
        // possible failure for this feature.
        Err(poisoned) => poisoned.into_inner(),
    };
    if let Some(info) = guard.as_ref() {
        return info.clone();
    }
    let info = voxctrl_bugreport::sysinfo::collect(
        &build_facts(),
        &voxctrl_bugreport::scrub::Scrubber::from_env(),
    );
    *guard = Some(info.clone());
    info
}

/// Read the routing files as JSON for redaction.
///
/// A file that will not parse is reported as such rather than dropped: "the
/// targets file is unreadable" is frequently the bug being reported.
fn routing_json() -> (Value, Value) {
    let dir = voxctrl_routing::config_dir();
    let targets = match voxctrl_routing::load_targets(&dir) {
        Ok(targets) => serde_json::to_value(targets).unwrap_or_else(|_| Value::Array(vec![])),
        Err(e) => Value::String(format!("<targets.toml could not be read: {e}>")),
    };
    let bindings = match voxctrl_routing::load_bindings(&dir) {
        Ok(bindings) => serde_json::to_value(bindings).unwrap_or_else(|_| Value::Array(vec![])),
        Err(e) => Value::String(format!("<bindings.toml could not be read: {e}>")),
    };
    (targets, bindings)
}

/// Read the history, and persist it only if reading it had to mint an
/// identifier.
///
/// Both halves matter. Persisting is what makes the identifier the page shows
/// before you type the same one that ends up in the report — without it, every
/// call would mint a fresh one and the two would disagree. Persisting *only*
/// when something changed is what keeps the preview, which reruns at every
/// pause in typing, from rewriting a file on each keystroke burst.
fn history_with_identity() -> (voxctrl_bugreport::History, String) {
    let mut history = voxctrl_bugreport::load_history();
    let was_empty = history.install_id.is_empty();
    let install_id = history.install_id().to_string();
    if was_empty {
        if let Err(e) = voxctrl_bugreport::save_history(&history) {
            tracing::warn!("Could not write the bug-report identity: {e}");
        }
    }
    (history, install_id)
}

/// Assemble the report for the statement the user has typed so far.
///
/// Everything after the config read happens on a blocking thread. The first
/// call gathers the machine facts, which on Windows means waiting on
/// PowerShell — several seconds of a runtime worker held still, during which
/// nothing else the app does with the async runtime would run. It is only
/// noticeable once per launch, but "only noticeable once" is how a frozen
/// window gets shipped.
async fn assemble(
    state: &State<'_, Arc<AppState>>,
    statement: UserStatement,
) -> Result<BugReport, String> {
    let config = {
        let guard = state.config.lock().await;
        serde_json::to_value(&guard.data).map_err(|e| format!("could not read settings: {e}"))?
    };

    tokio::task::spawn_blocking(move || {
        let (targets, bindings) = routing_json();
        let (_, install_id) = history_with_identity();
        voxctrl_bugreport::build(
            statement,
            install_id,
            Sources {
                system: cached_system_info(),
                config: &config,
                targets: &targets,
                bindings: &bindings,
            },
        )
    })
    .await
    .map_err(|e| format!("could not gather the report: {e}"))
}

// ── What the page needs to describe itself ───────────────────────────────────

#[derive(Debug, Serialize)]
pub struct LimitsView {
    pub cooldown_seconds: i64,
    pub per_day: usize,
    pub per_month: usize,
    pub min_description_chars: usize,
    pub max_description_chars: usize,
}

#[derive(Debug, Serialize)]
pub struct BugReportContext {
    /// Whether this build can send a report itself. When false the page offers
    /// only the routes that need no server, and says so.
    pub relay_configured: bool,
    pub issues_new_url: String,
    pub support_email: String,
    /// The log file a report quotes, so the user can go and read it first.
    pub log_path: String,
    pub install_id: String,
    pub limits: LimitsView,
    pub submissions_last_day: usize,
    pub submissions_last_month: usize,
}

#[tauri::command]
pub async fn bug_report_context() -> Result<BugReportContext, String> {
    let limits = Limits::default();
    let (history, install_id) = history_with_identity();
    let now = Utc::now();
    let within = |days: i64| {
        history
            .submissions
            .iter()
            .filter(|s| now - s.at < chrono::Duration::days(days) && s.at <= now)
            .count()
    };

    Ok(BugReportContext {
        relay_configured: submit::relay_endpoint().is_some(),
        issues_new_url: submit::ISSUES_NEW_URL.to_string(),
        support_email: submit::SUPPORT_EMAIL.to_string(),
        log_path: voxctrl_bugreport::logs::log_path()
            .to_string_lossy()
            .into_owned(),
        install_id,
        limits: LimitsView {
            cooldown_seconds: limits.cooldown.num_seconds(),
            per_day: limits.per_day,
            per_month: limits.per_month,
            min_description_chars: limits.min_description_chars,
            max_description_chars: limits.max_description_chars,
        },
        submissions_last_day: within(1),
        submissions_last_month: within(30),
    })
}

// ── Preview ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BugReportPreview {
    /// Exactly what would be sent, as Markdown. Not a summary of it.
    pub markdown: String,
    pub title: String,
    pub fingerprint: String,
    /// Set when the limits will not let this report be sent, with the sentence
    /// to show. The report can still be saved or filed by hand.
    pub blocked_reason: Option<String>,
    pub can_submit: bool,
    pub github_url: String,
    pub mailto_url: String,
}

#[tauri::command]
pub async fn preview_bug_report(
    state: State<'_, Arc<AppState>>,
    statement: UserStatement,
) -> Result<BugReportPreview, String> {
    let report = assemble(&state, statement).await?;
    let history = voxctrl_bugreport::load_history();
    let blocked = history
        .check(
            &report.statement.fingerprint_source(),
            &Limits::default(),
            Utc::now(),
        )
        .err();

    Ok(BugReportPreview {
        markdown: report.to_markdown(),
        title: report.issue_title(),
        fingerprint: report.fingerprint.clone(),
        can_submit: blocked.is_none() && submit::relay_endpoint().is_some(),
        blocked_reason: blocked.map(|refusal| refusal.message()),
        github_url: submit::github_issue_url(&report),
        mailto_url: submit::mailto_url(&report, None),
    })
}

// ── Sending ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BugReportOutcome {
    pub ok: bool,
    pub issue_url: Option<String>,
    pub message: String,
}

#[tauri::command]
pub async fn submit_bug_report(
    state: State<'_, Arc<AppState>>,
    statement: UserStatement,
) -> Result<BugReportOutcome, String> {
    let report = assemble(&state, statement).await?;

    // Re-checked here rather than trusting the preview: the page could have
    // been sitting open since before the last submission.
    let mut history = voxctrl_bugreport::load_history();
    let now = Utc::now();
    if let Err(refusal) = history.check(
        &report.statement.fingerprint_source(),
        &Limits::default(),
        now,
    ) {
        return Ok(BugReportOutcome {
            ok: false,
            issue_url: None,
            message: refusal.message(),
        });
    }

    match submit::submit(&report).await {
        Ok(response) => {
            // Recorded only on success. A report that never reached the relay
            // must not spend the reporter's daily allowance.
            history.record(&report.statement.fingerprint_source(), now);
            if let Err(e) = voxctrl_bugreport::save_history(&history) {
                tracing::warn!("Could not write the bug-report history: {e}");
            }
            let message = response.message.unwrap_or_else(|| {
                if response.duplicate {
                    "Thank you — this matches a report already filed, so it has been added to \
                     that one rather than opening a duplicate."
                        .into()
                } else {
                    "Thank you — your report has been filed.".into()
                }
            });
            tracing::info!("Bug report submitted");
            Ok(BugReportOutcome {
                ok: true,
                issue_url: response.issue_url,
                message,
            })
        }
        Err(e) => {
            tracing::warn!("Bug report could not be submitted: {e}");
            Ok(BugReportOutcome {
                ok: false,
                issue_url: None,
                message: format!(
                    "{e} Your report has not been lost — use “Save report to a file” below and \
                     send it along, or open it on GitHub."
                ),
            })
        }
    }
}

// ── The routes that need no server ───────────────────────────────────────────

#[tauri::command]
pub async fn save_bug_report(
    state: State<'_, Arc<AppState>>,
    statement: UserStatement,
    path: String,
) -> Result<String, String> {
    let report = assemble(&state, statement).await?;
    std::fs::write(&path, report.to_markdown())
        .map_err(|e| format!("could not write {path}: {e}"))?;
    tracing::info!("Bug report saved to a file");
    Ok(path)
}

/// A suggested filename for the save dialog.
#[tauri::command]
pub fn suggested_bug_report_filename() -> String {
    format!(
        "voxctrl-bug-report-{}.md",
        Utc::now().format("%Y%m%d-%H%M%S")
    )
}

/// Throw away the installation identifier and make a new one.
///
/// Offered because an identifier that cannot be reset is a tracking identifier
/// whatever it was meant to be. Resetting it also clears the local submission
/// history, since that history is what the old identifier indexed.
#[tauri::command]
pub async fn reset_bug_report_identity() -> Result<String, String> {
    let mut history = voxctrl_bugreport::History::default();
    let fresh = history.install_id().to_string();
    voxctrl_bugreport::save_history(&history)
        .map_err(|e| format!("could not reset the report identity: {e}"))?;
    tracing::info!("Bug-report identity reset at the user's request");
    Ok(fresh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_always_reports_at_least_one_feature() {
        // "none reported" in a report is ambiguous between a base build and a
        // collector that failed, so the base build says so in words.
        assert!(!build_features().is_empty());
    }

    #[test]
    fn the_suggested_filename_is_sortable_and_has_no_spaces() {
        let name = suggested_bug_report_filename();
        assert!(name.starts_with("voxctrl-bug-report-"));
        assert!(name.ends_with(".md"));
        assert!(!name.contains(' '), "a filename with spaces trips shells and links");
    }
}
