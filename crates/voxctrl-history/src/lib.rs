//! Dictation history: the last few things the user dictated, kept so they can
//! be found, pasted again, sent elsewhere or read aloud.
//!
//! Dictations can be private, so the design starts from "keep as little as
//! possible, as briefly as possible, where as little as possible can read it":
//!
//! - **Memory only by default.** [`History`] holds entries in this process's
//!   memory; nothing reaches disk unless the user turns on keeping history
//!   between restarts.
//! - **Encrypted when it does reach disk** ([`store`]): XChaCha20-Poly1305,
//!   with the key held by the operating system's keyring ([`keys`]), never in
//!   a file beside it, and the file readable by its owner only. With no
//!   keyring available, history is not written at all, rather than falling
//!   back to plain text.
//! - **Bounded**: a cap on entries and an age limit, enforced on every change.
//! - **Never logged.** Nothing in this crate writes entry text to logs.
//!
//! What this cannot do: a malicious program running as the same user can read
//! this process's memory or ask the unlocked keyring for the key. This guards
//! against everything short of that: other users, other apps scanning files,
//! backups and sync folders, and accidental leaks through logs.

use serde::{Deserialize, Serialize};

pub mod keys;
pub mod store;

/// One dictation, as delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Unique within a history; increases with each entry.
    pub id: u64,
    /// When it was delivered, in seconds since the Unix epoch.
    pub at: i64,
    /// Where it went: the output target's id(s), comma-separated when one
    /// dictation was sent to several.
    pub target_id: String,
    /// The target's name as shown to the user.
    pub target_label: String,
    /// The text that was delivered (after clean-up).
    pub text: String,
}

/// How much history to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// At most this many entries; the oldest go first. 0 keeps none.
    pub max_entries: usize,
    /// Entries older than this many seconds are dropped. 0 means no age limit.
    pub max_age_secs: i64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_entries: 50,
            max_age_secs: 24 * 60 * 60,
        }
    }
}

/// The history, newest last.
#[derive(Debug, Default)]
pub struct History {
    entries: Vec<Entry>,
    next_id: u64,
    limits: Limits,
}

impl History {
    pub fn new(limits: Limits) -> Self {
        Self {
            entries: Vec::new(),
            next_id: 1,
            limits,
        }
    }

    /// A history continuing from entries loaded from disk.
    pub fn from_entries(mut entries: Vec<Entry>, limits: Limits, now: i64) -> Self {
        entries.sort_by_key(|e| e.id);
        let next_id = entries.last().map_or(1, |e| e.id + 1);
        let mut h = Self {
            entries,
            next_id,
            limits,
        };
        h.prune(now);
        h
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Change the limits, dropping whatever no longer fits.
    pub fn set_limits(&mut self, limits: Limits, now: i64) {
        self.limits = limits;
        self.prune(now);
    }

    /// Add a dictation and return its id, or `None` when nothing is kept
    /// (blank text, or a cap of 0).
    pub fn add(&mut self, at: i64, target_id: &str, target_label: &str, text: &str) -> Option<u64> {
        if text.trim().is_empty() || self.limits.max_entries == 0 {
            return None;
        }
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.entries.push(Entry {
            id,
            at,
            target_id: target_id.to_string(),
            target_label: target_label.to_string(),
            text: text.to_string(),
        });
        self.prune(at);
        Some(id)
    }

    /// Drop entries past the age limit, then the oldest past the cap.
    /// True when anything was dropped.
    pub fn prune(&mut self, now: i64) -> bool {
        let before = self.entries.len();
        if self.limits.max_age_secs > 0 {
            let cutoff = now - self.limits.max_age_secs;
            self.entries.retain(|e| e.at >= cutoff);
        }
        let excess = self.entries.len().saturating_sub(self.limits.max_entries);
        if excess > 0 {
            self.entries.drain(..excess);
        }
        self.entries.len() != before
    }

    pub fn get(&self, id: u64) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn delete(&mut self, id: u64) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.id != id);
        self.entries.len() != before
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// All entries, oldest first.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Newest first, optionally only those that match `query`, at most
    /// `limit` of them. See [`matches`] for how a query matches.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Entry> {
        let terms = terms(query);
        self.entries
            .iter()
            .rev()
            .filter(|e| matches_terms(e, &terms))
            .take(limit)
            .cloned()
            .collect()
    }
}

/// Split a search query into the terms that must all match: words, or
/// phrases in double quotes ("send the report"), lower-cased. An unclosed
/// quote runs to the end of the query. The History window highlights matches
/// with the same rules (`src/lib/History/search.ts`).
pub fn terms(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (i, part) in query.split('"').enumerate() {
        if i % 2 == 1 {
            // Inside quotes: one phrase, with its spaces squeezed.
            let phrase = part.split_whitespace().collect::<Vec<_>>().join(" ");
            if !phrase.is_empty() {
                out.push(phrase.to_lowercase());
            }
        } else {
            out.extend(part.split_whitespace().map(str::to_lowercase));
        }
    }
    out
}

/// Whether an entry matches a search query: every term appears, in any
/// order and ignoring case, in its text or in the name of where it went. A
/// blank query matches everything.
pub fn matches(entry: &Entry, query: &str) -> bool {
    matches_terms(entry, &terms(query))
}

fn matches_terms(entry: &Entry, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    let text = entry.text.to_lowercase();
    let target = entry.target_label.to_lowercase();
    terms
        .iter()
        .all(|t| text.contains(t.as_str()) || target.contains(t.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: i64 = 3600;

    fn limits(max_entries: usize, max_age_secs: i64) -> Limits {
        Limits {
            max_entries,
            max_age_secs,
        }
    }

    #[test]
    fn keeps_entries_newest_first_in_search() {
        let mut h = History::new(Limits::default());
        h.add(100, "default", "Focused Window", "first");
        h.add(200, "notes", "Notes", "second");
        let found: Vec<_> = h.search("", 10).into_iter().map(|e| e.text).collect();
        assert_eq!(found, ["second", "first"]);
    }

    #[test]
    fn blank_text_is_not_kept() {
        let mut h = History::new(Limits::default());
        assert_eq!(h.add(1, "t", "T", "   "), None);
        assert!(h.is_empty());
    }

    #[test]
    fn a_cap_of_zero_keeps_nothing() {
        let mut h = History::new(limits(0, 0));
        assert_eq!(h.add(1, "t", "T", "secret"), None);
        assert!(h.is_empty());
    }

    #[test]
    fn the_oldest_entries_go_past_the_cap() {
        let mut h = History::new(limits(3, 0));
        for i in 0..5 {
            h.add(i, "t", "T", &format!("n{i}"));
        }
        let kept: Vec<_> = h.entries().iter().map(|e| e.text.as_str()).collect();
        assert_eq!(kept, ["n2", "n3", "n4"]);
    }

    #[test]
    fn entries_expire_after_the_age_limit() {
        let mut h = History::new(limits(50, 24 * HOUR));
        h.add(0, "t", "T", "old");
        h.add(10 * HOUR, "t", "T", "newer");
        assert!(h.prune(25 * HOUR));
        assert_eq!(h.search("", 10).len(), 1);
        assert!(h.prune(40 * HOUR));
        assert!(h.is_empty());
    }

    #[test]
    fn tightening_the_limits_drops_entries_at_once() {
        let mut h = History::new(limits(50, 0));
        for i in 0..10 {
            h.add(i, "t", "T", "x");
        }
        h.set_limits(limits(4, 0), 10);
        assert_eq!(h.len(), 4);
    }

    #[test]
    fn search_matches_text_or_target_ignoring_case() {
        let mut h = History::new(Limits::default());
        h.add(
            1,
            "default",
            "Focused Window",
            "Push the release to Thursday",
        );
        h.add(2, "notes", "Meeting Notes", "groceries");
        assert_eq!(h.search("thursday", 10).len(), 1);
        assert_eq!(h.search("meeting", 10)[0].text, "groceries");
        assert!(h.search("nothing", 10).is_empty());
        assert_eq!(h.search("", 1).len(), 1);
    }

    #[test]
    fn every_word_must_match_in_any_order() {
        let mut h = History::new(Limits::default());
        h.add(
            1,
            "default",
            "Focused Window",
            "Push the release to Thursday",
        );
        h.add(2, "notes", "Meeting Notes", "Thursday groceries");
        assert_eq!(h.search("thursday release", 10)[0].id, 1);
        assert_eq!(h.search("RELEASE   thursday", 10).len(), 1);
        // A word can match the target name while another matches the text.
        assert_eq!(h.search("notes thursday", 10)[0].id, 2);
        assert!(h.search("thursday friday", 10).is_empty());
    }

    #[test]
    fn quoted_phrases_must_match_together() {
        let mut h = History::new(Limits::default());
        h.add(1, "t", "T", "send the report to Dana");
        h.add(2, "t", "T", "the report can wait, send it later");
        assert_eq!(h.search("send report", 10).len(), 2);
        let found = h.search("\"send the  report\"", 10);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, 1);
        // An unclosed quote runs to the end.
        assert_eq!(h.search("dana \"the report", 10).len(), 1);
    }

    #[test]
    fn terms_splits_words_and_phrases() {
        assert_eq!(
            terms("  Hello \"New  York\" there "),
            ["hello", "new york", "there"]
        );
        assert!(terms("  \"  \" ").is_empty());
    }

    #[test]
    fn delete_and_clear() {
        let mut h = History::new(Limits::default());
        let a = h.add(1, "t", "T", "a").unwrap();
        h.add(2, "t", "T", "b");
        assert!(h.delete(a));
        assert!(!h.delete(a));
        assert_eq!(h.len(), 1);
        h.clear();
        assert!(h.is_empty());
    }

    #[test]
    fn ids_keep_increasing_after_loading_from_disk() {
        let loaded = vec![
            Entry {
                id: 7,
                at: 100,
                target_id: "t".into(),
                target_label: "T".into(),
                text: "a".into(),
            },
            Entry {
                id: 3,
                at: 50,
                target_id: "t".into(),
                target_label: "T".into(),
                text: "b".into(),
            },
        ];
        let mut h = History::from_entries(loaded, limits(50, 0), 200);
        assert_eq!(h.entries()[0].id, 3);
        assert_eq!(h.add(201, "t", "T", "c"), Some(8));
    }
}
