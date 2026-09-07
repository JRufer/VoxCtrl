//! Getting a finished report to the maintainer.
//!
//! There are three routes, and the reason there are three is worth stating
//! plainly, because it is the constraint that shaped this whole feature:
//!
//! **GitHub has no anonymous issue creation.** `POST /repos/{owner}/{repo}/issues`
//! requires a credential, and there is no unauthenticated equivalent. Shipping a
//! token inside the app is not an option — a token in a binary is a token in
//! everyone's hands, and the first thing it would be used for is filing the
//! spam this feature is meant to avoid.
//!
//! So:
//!
//! 1. [`submit`] posts the report to a **relay** the maintainer runs, which
//!    holds the GitHub credential, applies the abuse limits that actually bind,
//!    and opens the issue itself. The reporter needs no account.
//!    (`scripts/bug-report-relay/` is a complete one.)
//! 2. [`github_issue_url`] opens GitHub's new-issue form with everything
//!    filled in, for a reporter who does have an account. Nothing is sent
//!    anywhere until they press Submit on GitHub's own page.
//! 3. [`mailto_url`], plus saving the report to a file, for when there is no
//!    relay deployed, no account, or no wish to use either.
//!
//! Routes 2 and 3 need no infrastructure at all and work the day this ships.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::report::{truncate_chars, BugReport};

/// Where a report goes when the user presses Send.
///
/// Baked in at build time so a stray config edit cannot redirect reports to
/// somewhere else, and left unset in builds where no relay is deployed — in
/// which case the Bug Report page offers only the routes that need no server.
///
/// Read through `build.rs` rather than with `option_env!` directly; see that
/// file for why the direct spelling silently keeps a stale value.
///
/// The empty string means "no relay". An unset repository secret in a CI
/// workflow arrives as an empty variable rather than an absent one, so
/// treating empty as absent is what keeps a build without the secret from
/// advertising a Send button that points nowhere.
pub fn relay_endpoint() -> Option<&'static str> {
    Some(env!("VOXCTRL_BUGREPORT_ENDPOINT_BAKED")).filter(|url| !url.is_empty())
}

/// The repository issues are filed against.
pub const ISSUES_NEW_URL: &str = "https://github.com/JRufer/VoxCtrl/issues/new";

/// Where a report can be emailed when there is no relay and no GitHub account.
pub const SUPPORT_EMAIL: &str = "voxctrl-bugs@proton.me";

/// GitHub answers a `GET` longer than about 8 KB with an error page rather than
/// the form, so the prefilled body is trimmed well inside that.
const MAX_URL_BODY_CHARS: usize = 5500;

/// A mail client's command line is shorter still, and a body that overflows it
/// is silently cut — so the email route carries a pointer to the saved file
/// rather than the report.
const MAX_MAILTO_BODY_CHARS: usize = 1500;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, thiserror::Error)]
pub enum SubmitError {
    #[error("no bug-report relay is configured in this build")]
    NotConfigured,
    #[error("could not reach the bug-report service: {0}")]
    Network(String),
    /// The relay refused, and said why in words meant for the user — most often
    /// its own rate limit.
    #[error("{0}")]
    Refused(String),
    #[error("the bug-report service returned something unexpected: {0}")]
    Unexpected(String),
}

/// What is POSTed to the relay.
///
/// The rendered title and body travel alongside the structured report so the
/// relay can file an issue without reimplementing the renderer — and so what
/// gets filed is character-for-character what the reporter read in the preview.
/// The renderer already defuses mentions and cross-references, so the relay's
/// job is limits and size, not sanitising.
#[derive(Debug, Clone, Serialize)]
pub struct SubmitEnvelope<'a> {
    pub schema: u32,
    pub title: String,
    pub body: String,
    pub fingerprint: &'a str,
    pub install_id: &'a str,
    pub app_version: &'a str,
    pub os: &'a str,
    pub report: &'a BugReport,
}

impl<'a> SubmitEnvelope<'a> {
    pub fn new(report: &'a BugReport) -> Self {
        Self {
            schema: report.schema,
            title: report.issue_title(),
            body: report.to_markdown(),
            fingerprint: &report.fingerprint,
            install_id: &report.install_id,
            app_version: &report.system.app_version,
            os: &report.system.os,
            report,
        }
    }
}

/// What the relay sends back.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SubmitResponse {
    /// The issue that was opened, when one was.
    #[serde(default)]
    pub issue_url: Option<String>,
    /// A sentence for the user. The relay writes it, so a maintainer can
    /// explain a refusal without shipping a new build.
    #[serde(default)]
    pub message: Option<String>,
    /// True when the relay recognised the report as one already filed.
    #[serde(default)]
    pub duplicate: bool,
}

/// Post a report to the relay.
pub async fn submit(report: &BugReport) -> Result<SubmitResponse, SubmitError> {
    let endpoint = relay_endpoint().ok_or(SubmitError::NotConfigured)?;
    let client = reqwest::Client::builder()
        .user_agent(concat!("VoxCtrl/", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| SubmitError::Network(e.to_string()))?;

    let response = client
        .post(endpoint)
        .header("X-VoxCtrl-Version", &report.system.app_version)
        .header("X-VoxCtrl-Schema", report.schema.to_string())
        .json(&SubmitEnvelope::new(report))
        .send()
        .await
        .map_err(|e| SubmitError::Network(e.to_string()))?;

    interpret(response.status().as_u16(), &response.text().await.unwrap_or_default())
}

/// Turn the relay's status and body into a result.
///
/// Separated from the request so the contract with the relay is testable
/// without one running.
pub fn interpret(status: u16, body: &str) -> Result<SubmitResponse, SubmitError> {
    let parsed: Option<SubmitResponse> = serde_json::from_str(body).ok();

    match status {
        200..=299 => Ok(parsed.unwrap_or_default()),
        // The relay's own rate limit, and the reason it is the one that counts:
        // it sees every reporter, not just this installation.
        429 => Err(SubmitError::Refused(
            parsed
                .and_then(|r| r.message)
                .unwrap_or_else(|| {
                    "The bug-report service is rate-limiting reports right now. Please try again \
                     later, or save the report to a file and send it directly."
                        .into()
                }),
        )),
        400..=499 => Err(SubmitError::Refused(
            parsed.and_then(|r| r.message).unwrap_or_else(|| {
                format!("The bug-report service would not accept this report (HTTP {status}).")
            }),
        )),
        _ => Err(SubmitError::Unexpected(format!(
            "HTTP {status}{}",
            if body.is_empty() {
                String::new()
            } else {
                format!(": {}", truncate_chars(body, 200))
            }
        ))),
    }
}

/// GitHub's new-issue form, prefilled.
///
/// Opening this sends nothing: it is a form on GitHub, filled in, waiting for
/// the reporter to read it and press Submit. That is worth saying in the UI,
/// because "open a GitHub issue" sounds like it files one.
pub fn github_issue_url(report: &BugReport) -> String {
    let body = report.to_markdown();
    let trimmed = if body.chars().count() > MAX_URL_BODY_CHARS {
        format!(
            "{}\n\n_The rest of this report did not fit in a link. Use **Copy report** on \
             VoxCtrl's Bug Report page and paste it here to include all of it._\n",
            truncate_chars(&body, MAX_URL_BODY_CHARS)
        )
    } else {
        body
    };
    format!(
        "{ISSUES_NEW_URL}?labels=bug&title={}&body={}",
        percent_encode(&report.issue_title()),
        percent_encode(&trimmed)
    )
}

/// A prefilled email, for reporters with neither a relay nor an account.
///
/// The body is a covering note, not the report: mail clients truncate long
/// `mailto:` bodies without saying so, and a silently halved report is worse
/// than none. The report goes as the saved file, attached by hand.
pub fn mailto_url(report: &BugReport, saved_file: Option<&str>) -> String {
    let attachment_line = match saved_file {
        Some(path) => format!("The full report is saved at:\n{path}\nPlease attach that file."),
        None => "Use \"Save report to a file\" on VoxCtrl's Bug Report page and attach the file \
                 it writes."
            .to_string(),
    };
    let body = format!(
        "{}\n\n---\n{attachment_line}\n\nVoxCtrl {} on {} ({})\nReport ID: {}\n",
        truncate_chars(report.statement.description.trim(), MAX_MAILTO_BODY_CHARS),
        report.system.app_version,
        report.system.os,
        report.system.install_kind,
        &report.fingerprint[..12.min(report.fingerprint.len())],
    );
    format!(
        "mailto:{SUPPORT_EMAIL}?subject={}&body={}",
        percent_encode(&report.issue_title()),
        percent_encode(&body)
    )
}

/// Percent-encode for a query string.
///
/// Only the RFC 3986 unreserved set survives; everything else, space included,
/// is escaped. Space becomes `%20` rather than `+`, because `mailto:` bodies
/// are not form-encoded and a `+` there arrives as a literal plus sign.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{LogSection, UserStatement, SCHEMA_VERSION};
    use crate::sysinfo::SystemInfo;
    use chrono::DateTime;
    use serde_json::json;

    fn report() -> BugReport {
        BugReport {
            schema: SCHEMA_VERSION,
            created_at: DateTime::from_timestamp(1_800_000_000, 0).unwrap(),
            install_id: "0123456789abcdef0123456789abcdef".into(),
            fingerprint: "abcdef0123456789abcdef".into(),
            statement: UserStatement {
                summary: "Recording stops & starts".into(),
                description: "It records for a second then stops.".into(),
                area: "Hotkeys".into(),
                frequency: "always".into(),
            },
            system: SystemInfo {
                app_version: "0.5.1".into(),
                install_kind: "installed".into(),
                os: "windows".into(),
                arch: "x86_64".into(),
                ..SystemInfo::default()
            },
            config: json!({}),
            targets: json!([]),
            bindings: json!([]),
            log: LogSection { text: "log".into(), lines: 1, truncated: false },
        }
    }

    #[test]
    fn the_envelope_carries_the_body_the_reporter_was_shown() {
        let r = report();
        let envelope = SubmitEnvelope::new(&r);
        assert_eq!(envelope.title, r.issue_title());
        assert_eq!(envelope.body, r.to_markdown());
        assert_eq!(envelope.schema, r.schema);
        // And the structured report travels too, so the relay can triage on
        // fields rather than parsing prose.
        let json = serde_json::to_value(&envelope).unwrap();
        assert_eq!(json["report"]["system"]["os"], "windows");
    }

    #[test]
    fn a_successful_relay_reply_carries_the_issue_url() {
        let response = interpret(201, r#"{"issue_url":"https://github.com/o/r/issues/7"}"#).unwrap();
        assert_eq!(
            response.issue_url.as_deref(),
            Some("https://github.com/o/r/issues/7")
        );
    }

    #[test]
    fn a_relay_rate_limit_reaches_the_user_in_the_relays_own_words() {
        let err = interpret(429, r#"{"message":"Three reports an hour, please."}"#).unwrap_err();
        assert_eq!(err.to_string(), "Three reports an hour, please.");
    }

    #[test]
    fn a_rate_limit_with_no_body_still_explains_itself() {
        let err = interpret(429, "").unwrap_err();
        assert!(err.to_string().contains("save the report to a file"));
    }

    #[test]
    fn a_server_error_is_not_dressed_up_as_a_refusal() {
        // A 500 is the relay's fault, not the reporter's, and telling them
        // they were refused would send them away for no reason.
        let err = interpret(500, "gateway exploded").unwrap_err();
        assert!(matches!(err, SubmitError::Unexpected(_)));
    }

    #[test]
    fn a_success_with_an_unreadable_body_is_still_a_success() {
        // The issue was opened; a relay that answers with an empty 200 has
        // done its job, and reporting failure would produce a duplicate.
        assert!(interpret(200, "").is_ok());
    }

    #[test]
    fn the_github_url_escapes_everything_that_would_break_it() {
        let url = github_issue_url(&report());
        assert!(url.starts_with(ISSUES_NEW_URL));
        // The ampersand in the summary must not start a new query parameter.
        assert!(url.contains("Recording%20stops%20%26%20starts"));
        assert!(!url[ISSUES_NEW_URL.len() + 1..].contains("& "));
    }

    #[test]
    fn a_report_too_big_for_a_url_is_cut_with_a_pointer_to_the_rest() {
        let mut r = report();
        r.statement.description = "x".repeat(20_000);
        let url = github_issue_url(&r);
        assert!(url.len() < 24_000, "url was {} bytes", url.len());
        assert!(percent_decode(&url).contains("did not fit in a link"));
    }

    #[test]
    fn the_email_route_points_at_the_file_rather_than_inlining_it() {
        let url = mailto_url(&report(), Some("/tmp/voxctrl-report.md"));
        let decoded = percent_decode(&url);
        assert!(decoded.starts_with("mailto:"));
        assert!(decoded.contains("/tmp/voxctrl-report.md"));
        assert!(decoded.contains("Please attach that file"));
    }

    #[test]
    fn spaces_in_a_mailto_body_are_percent_escaped_not_turned_into_plus_signs() {
        // A `+` in a mailto body arrives as a literal plus, which turns the
        // covering note into gibberish.
        let url = mailto_url(&report(), None);
        assert!(!url.contains('+'));
        assert!(url.contains("%20"));
    }

    /// Enough of a decoder to assert on what a URL actually carries.
    fn percent_decode(url: &str) -> String {
        let bytes = url.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                if let Ok(byte) = u8::from_str_radix(&url[i + 1..i + 3], 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
            out.push(bytes[i]);
            i += 1;
        }
        String::from_utf8_lossy(&out).into_owned()
    }
}
