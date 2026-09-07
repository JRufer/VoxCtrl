//! Keeping one person from filing the same report fifty times.
//!
//! Two honest words about what this is. Everything here runs on the reporter's
//! own machine, in a file they can delete, in a binary they can patch. It stops
//! an accident — a stuck button, a frustrated user pressing Submit until
//! something happens, the same crash reported every launch — and it stops
//! nothing at all from someone who means harm.
//!
//! The defence against that person is on the relay, which holds the GitHub
//! credential and rate-limits by source address before it will open an issue
//! (`scripts/bug-report-relay/`). This module is the courtesy layer: it tells a
//! user *why* the button is not doing anything, which the relay's silent 429
//! cannot.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The limits a build ships with.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Minimum wait between two submissions.
    pub cooldown: Duration,
    /// Submissions allowed in any rolling 24 hours.
    pub per_day: usize,
    /// Submissions allowed in any rolling 30 days.
    pub per_month: usize,
    /// A description shorter than this is not a bug report, it is a stray
    /// keypress — and it is the single most common thing an issue tracker
    /// fills up with.
    pub min_description_chars: usize,
    /// And a ceiling, so a pasted file cannot become an issue body.
    pub max_description_chars: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            cooldown: Duration::minutes(2),
            per_day: 5,
            per_month: 20,
            min_description_chars: 30,
            max_description_chars: 4000,
        }
    }
}

/// One past submission. Deliberately not a copy of the report: the fingerprint
/// is a hash, so the history file cannot become a second place a user's words
/// are stored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PastSubmission {
    pub at: DateTime<Utc>,
    pub fingerprint: String,
}

/// The on-disk record, next to the log the reports quote.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    /// A random identifier for this installation, generated on first use.
    ///
    /// It exists so the relay can rate-limit an installation that is behind a
    /// shared address, and so a duplicate can be recognised across restarts. It
    /// is derived from nothing about the machine or the person — see
    /// [`new_install_id`] — and the Bug Report page has a button that throws it
    /// away and makes a new one.
    #[serde(default)]
    pub install_id: String,
    #[serde(default)]
    pub submissions: Vec<PastSubmission>,
}

/// Why a submission is not going out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    TooShort { minimum: usize },
    TooLong { maximum: usize },
    Cooldown { seconds_remaining: i64 },
    DailyLimit { limit: usize, hours_until_next: i64 },
    MonthlyLimit { limit: usize },
    Duplicate { first_sent: DateTime<Utc> },
}

impl Refusal {
    /// What the user is told. Written to be read by someone who is already
    /// annoyed — it says what happened and when they can try again, and never
    /// implies they did something wrong.
    pub fn message(&self) -> String {
        match self {
            Refusal::TooShort { minimum } => format!(
                "Please describe the problem in at least {minimum} characters — what you did, \
                 what happened, and what you expected instead. That is usually the difference \
                 between a bug that gets fixed and one that cannot be reproduced."
            ),
            Refusal::TooLong { maximum } => format!(
                "That description is longer than {maximum} characters. Trim it to the essentials — \
                 the logs and settings are attached separately, so you do not need to paste them here."
            ),
            Refusal::Cooldown { seconds_remaining } => format!(
                "A report was just sent. You can send another in {seconds_remaining} seconds — \
                 this is only here so a stuck button cannot file the same thing repeatedly."
            ),
            Refusal::DailyLimit { limit, hours_until_next } => format!(
                "That is {limit} reports in 24 hours, which is the limit. The next slot opens in \
                 about {hours_until_next} hour(s). If something is badly broken and you have more \
                 to add, save the report to a file instead and send it along."
            ),
            Refusal::MonthlyLimit { limit } => format!(
                "That is {limit} reports in 30 days, which is the limit. Save the report to a file \
                 and send it directly instead — see the options below."
            ),
            Refusal::Duplicate { first_sent } => format!(
                "This looks like the report already sent on {}. Sending it again will not add \
                 anything; if you have new information, add it to the description and it will go \
                 through.",
                first_sent.format("%Y-%m-%d %H:%M UTC")
            ),
        }
    }
}

impl History {
    /// Whether this description may be submitted now.
    pub fn check(
        &self,
        description: &str,
        limits: &Limits,
        now: DateTime<Utc>,
    ) -> Result<(), Refusal> {
        let length = description.trim().chars().count();
        if length < limits.min_description_chars {
            return Err(Refusal::TooShort {
                minimum: limits.min_description_chars,
            });
        }
        if length > limits.max_description_chars {
            return Err(Refusal::TooLong {
                maximum: limits.max_description_chars,
            });
        }

        // A timestamp after `now` means the clock moved backwards since it was
        // written — a timezone change, an NTP correction, or someone editing
        // the file. Counting it as "just now" keeps the limits enforced instead
        // of turning a clock change into an unlimited allowance.
        let elapsed = |at: DateTime<Utc>| -> Duration {
            let gap = now - at;
            if gap < Duration::zero() {
                Duration::zero()
            } else {
                gap
            }
        };

        if let Some(latest) = self.submissions.iter().map(|s| s.at).max() {
            let since = elapsed(latest);
            if since < limits.cooldown {
                return Err(Refusal::Cooldown {
                    seconds_remaining: (limits.cooldown - since).num_seconds().max(1),
                });
            }
        }

        let day = Duration::days(1);
        let in_last_day: Vec<&PastSubmission> = self
            .submissions
            .iter()
            .filter(|s| elapsed(s.at) < day)
            .collect();
        if in_last_day.len() >= limits.per_day {
            // The oldest submission inside the window is the one whose expiry
            // frees the next slot.
            let oldest = in_last_day.iter().map(|s| s.at).min().unwrap_or(now);
            let wait = day - elapsed(oldest);
            return Err(Refusal::DailyLimit {
                limit: limits.per_day,
                hours_until_next: wait.num_hours().max(1),
            });
        }

        let month = Duration::days(30);
        if self
            .submissions
            .iter()
            .filter(|s| elapsed(s.at) < month)
            .count()
            >= limits.per_month
        {
            return Err(Refusal::MonthlyLimit {
                limit: limits.per_month,
            });
        }

        let fingerprint = fingerprint(description);
        if let Some(previous) = self
            .submissions
            .iter()
            .filter(|s| s.fingerprint == fingerprint)
            .max_by_key(|s| s.at)
        {
            return Err(Refusal::Duplicate {
                first_sent: previous.at,
            });
        }

        Ok(())
    }

    /// Record a submission that went out, and forget the ones that no longer
    /// bear on any limit.
    pub fn record(&mut self, description: &str, now: DateTime<Utc>) {
        self.submissions.push(PastSubmission {
            at: now,
            fingerprint: fingerprint(description),
        });
        self.prune(now);
    }

    /// Drop entries older than the longest window, so the file cannot grow
    /// without bound on a machine that has run for years.
    pub fn prune(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::days(31);
        self.submissions.retain(|s| s.at >= cutoff);
    }

    /// The install identifier, creating one on first use.
    pub fn install_id(&mut self) -> &str {
        if self.install_id.is_empty() {
            self.install_id = new_install_id();
        }
        &self.install_id
    }
}

/// A hash of the description, insensitive to whitespace and case.
///
/// "IT CRASHED!!!" and "it crashed" are the same report sent twice, and the
/// second one helps nobody.
pub fn fingerprint(description: &str) -> String {
    let normalized: String = description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    hex::encode(Sha256::digest(normalized.as_bytes()))
}

/// A fresh, random-enough installation identifier.
///
/// Random-*enough* is the requirement, and it is worth being precise about why
/// this is not a cryptographic random number: the only thing this value must do
/// is differ between installations so a rate limiter can tell them apart. It
/// must specifically **not** be derived from anything about the machine or the
/// person, because a value derived from those would follow them across
/// reinstalls and would be exactly the tracking identifier this app promises
/// not to have. Hashing the wall clock, the process id and this process's own
/// stack address gives a value that is different every time and traceable to
/// nothing.
pub fn new_install_id() -> String {
    let mut hasher = Sha256::new();
    hasher.update(Utc::now().timestamp_nanos_opt().unwrap_or_default().to_le_bytes());
    hasher.update(std::process::id().to_le_bytes());
    let stack_marker = 0u8;
    hasher.update((&stack_marker as *const u8 as usize).to_le_bytes());
    hex::encode(&hasher.finalize()[..16])
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "Recording stops after about a second on the Windows CPU build, \
                        and the overlay stays on screen afterwards.";

    fn at(minutes: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_800_000_000, 0).unwrap() + Duration::minutes(minutes)
    }

    #[test]
    fn a_first_report_goes_through() {
        assert_eq!(History::default().check(GOOD, &Limits::default(), at(0)), Ok(()));
    }

    #[test]
    fn a_one_word_report_is_refused_with_a_reason() {
        let refusal = History::default()
            .check("broken", &Limits::default(), at(0))
            .unwrap_err();
        assert!(matches!(refusal, Refusal::TooShort { .. }));
        assert!(refusal.message().contains("what you expected"));
    }

    #[test]
    fn a_pasted_novel_is_refused() {
        let long = "x".repeat(5000);
        assert!(matches!(
            History::default().check(&long, &Limits::default(), at(0)),
            Err(Refusal::TooLong { .. })
        ));
    }

    #[test]
    fn a_second_report_within_the_cooldown_waits() {
        let mut history = History::default();
        history.record(GOOD, at(0));
        let refusal = history
            .check("A different problem entirely, with plenty of detail here.", &Limits::default(), at(1))
            .unwrap_err();
        match refusal {
            Refusal::Cooldown { seconds_remaining } => assert_eq!(seconds_remaining, 60),
            other => panic!("expected a cooldown, got {other:?}"),
        }
    }

    #[test]
    fn the_same_report_twice_is_recognised_however_it_is_typed() {
        let mut history = History::default();
        history.record(GOOD, at(0));
        let shouted = format!("  {}  ", GOOD.to_uppercase());
        assert!(matches!(
            history.check(&shouted, &Limits::default(), at(60)),
            Err(Refusal::Duplicate { .. })
        ));
    }

    #[test]
    fn a_different_report_after_the_cooldown_goes_through() {
        let mut history = History::default();
        history.record(GOOD, at(0));
        assert_eq!(
            history.check(
                "Separate problem: the tray icon disappears after the display sleeps.",
                &Limits::default(),
                at(10)
            ),
            Ok(())
        );
    }

    #[test]
    fn the_daily_limit_says_when_the_next_slot_opens() {
        let limits = Limits::default();
        let mut history = History::default();
        for n in 0..limits.per_day {
            history.record(&format!("{GOOD} variation {n}"), at(n as i64 * 10));
        }
        let refusal = history
            .check("Yet another distinct problem, described at length.", &limits, at(300))
            .unwrap_err();
        match refusal {
            Refusal::DailyLimit { limit, hours_until_next } => {
                assert_eq!(limit, limits.per_day);
                assert!((1..=24).contains(&hours_until_next), "got {hours_until_next}");
            }
            other => panic!("expected the daily limit, got {other:?}"),
        }
    }

    #[test]
    fn yesterdays_reports_do_not_count_against_today() {
        let limits = Limits::default();
        let mut history = History::default();
        for n in 0..limits.per_day {
            history.record(&format!("{GOOD} variation {n}"), at(n as i64));
        }
        // A day and a bit later the window has rolled past all of them.
        assert_eq!(
            history.check("A new problem, freshly described in detail.", &limits, at(60 * 25)),
            Ok(())
        );
    }

    #[test]
    fn a_clock_moved_backwards_does_not_reset_the_limits() {
        // Someone who sets their clock back — or whose machine corrects itself
        // over NTP — should not get a fresh allowance. The submission is
        // treated as having just happened, so the cooldown still applies.
        let mut history = History::default();
        history.record(GOOD, at(0));
        let refusal = history
            .check("Something else, described at sufficient length to pass.", &Limits::default(), at(-500))
            .unwrap_err();
        assert!(matches!(refusal, Refusal::Cooldown { .. }));
    }

    #[test]
    fn old_entries_are_pruned_so_the_file_cannot_grow_forever() {
        let mut history = History::default();
        history.record(GOOD, at(-60 * 24 * 40));
        history.record("recent enough to keep, with a full description here", at(0));
        assert_eq!(history.submissions.len(), 1);
    }

    #[test]
    fn an_install_id_is_made_once_and_then_kept() {
        let mut history = History::default();
        let first = history.install_id().to_string();
        assert_eq!(first.len(), 32);
        assert_eq!(history.install_id(), first);
        assert_ne!(first, new_install_id(), "each install gets its own");
    }

    #[test]
    fn the_history_file_holds_no_words_the_user_typed() {
        // The whole point of storing a hash: the record of what was reported
        // must not itself become a place someone's bug descriptions pile up.
        let mut history = History::default();
        history.record("my password is hunter2 and the app crashed", at(0));
        let stored = serde_json::to_string(&history).unwrap();
        assert!(!stored.contains("hunter2"));
        assert!(!stored.contains("password"));
    }
}
