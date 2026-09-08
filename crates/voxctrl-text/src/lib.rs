//! Shared text-processing helpers used by both the speech-recognition
//! post-processing pipeline (`voxctrl-inference`) and the TTS engine
//! (`voxctrl-tts`). Both crates apply the same snippet expansion and
//! custom-vocabulary fuzzy correction to user text — this crate is the
//! single source of truth for that logic so a fix or tuning change only
//! needs to happen once.

#[cfg(test)]
mod reference;

use std::collections::HashMap;

use regex::Regex;

/// Classic Levenshtein (edit) distance between two strings, computed over
/// `char`s rather than bytes so it works correctly with non-ASCII input.
pub fn levenshtein_distance(s1: &str, s2: &str) -> usize {
    distance_within(s1, s2, usize::MAX).unwrap_or(usize::MAX)
}

/// Whether `s1` and `s2` are within `max` edits of each other.
fn within(s1: &str, s2: &str, max: usize) -> bool {
    distance_within(s1, s2, max).is_some()
}

thread_local! {
    /// Scratch space for the distance routine: the two character buffers and
    /// the two DP rows. Vocabulary correction runs this once per word per
    /// vocabulary entry, so allocating four buffers per call would cost more
    /// than the arithmetic it exists for.
    static SCRATCH: std::cell::RefCell<Scratch> = const {
        std::cell::RefCell::new(Scratch {
            a: Vec::new(),
            b: Vec::new(),
            prev: Vec::new(),
            cur: Vec::new(),
        })
    };
}

struct Scratch {
    a: Vec<char>,
    b: Vec<char>,
    prev: Vec<usize>,
    cur: Vec<usize>,
}

/// Levenshtein distance between `s1` and `s2`, abandoned as soon as every
/// path through the table is known to exceed `max`.
///
/// Returns `None` when the distance is greater than `max`. That early exit is
/// what makes vocabulary correction cheap: nearly every comparison is between
/// two unrelated words, and those are rejected after a row or two instead of
/// filling the whole table.
fn distance_within(s1: &str, s2: &str, max: usize) -> Option<usize> {
    SCRATCH.with(|cell| match cell.try_borrow_mut() {
        Ok(mut guard) => {
            let s = &mut *guard;
            s.a.clear();
            s.a.extend(s1.chars());
            s.b.clear();
            s.b.extend(s2.chars());
            distance_rows(&s.a, &s.b, max, &mut s.prev, &mut s.cur)
        }
        // A re-entrant call (there is none today) would find the buffers
        // borrowed; fall back to fresh ones rather than panicking.
        Err(_) => {
            let (a, b): (Vec<char>, Vec<char>) = (s1.chars().collect(), s2.chars().collect());
            distance_rows(&a, &b, max, &mut Vec::new(), &mut Vec::new())
        }
    })
}

fn distance_rows(
    a: &[char],
    b: &[char],
    max: usize,
    prev: &mut Vec<usize>,
    cur: &mut Vec<usize>,
) -> Option<usize> {
    if a.len().abs_diff(b.len()) > max {
        return None;
    }
    if a.is_empty() {
        return (b.len() <= max).then_some(b.len());
    }
    if b.is_empty() {
        return (a.len() <= max).then_some(a.len());
    }

    prev.clear();
    prev.extend(0..=b.len());
    cur.clear();
    cur.resize(b.len() + 1, 0);

    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let d = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            cur[j + 1] = d;
            row_min = row_min.min(d);
        }
        if row_min > max {
            return None;
        }
        std::mem::swap(prev, cur);
    }
    let d = prev[b.len()];
    (d <= max).then_some(d)
}

/// Replaces short trigger words/phrases in `text` with their configured
/// expansions (case-insensitive, word-boundary matched). Used for both
/// dictation snippets (STT output) and spoken-text shortcuts (TTS input).
pub fn expand_snippets(text: &str, snippets: &HashMap<String, String>) -> String {
    if snippets.is_empty() {
        return text.to_string();
    }
    let mut result = std::borrow::Cow::Borrowed(text);
    for (trigger, expansion) in snippets {
        with_trigger_regex(trigger, |re| {
            if let Some(re) = re {
                if re.is_match(&result) {
                    result = std::borrow::Cow::Owned(
                        re.replace_all(&result, expansion.as_str()).into_owned(),
                    );
                }
            }
        });
    }
    result.into_owned()
}

/// Hands `f` the compiled word-boundary regex for `trigger`, compiling it on
/// first use and keeping it for the rest of the session.
///
/// Snippet triggers change only when the user edits their settings, so
/// recompiling every one of them on every transcription — regex compilation
/// costs orders of magnitude more than the match itself — was pure waste.
fn with_trigger_regex<R>(trigger: &str, f: impl FnOnce(Option<&Regex>) -> R) -> R {
    static CACHE: std::sync::OnceLock<std::sync::RwLock<HashMap<String, Option<Regex>>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);

    if let Ok(map) = cache.read() {
        if let Some(re) = map.get(trigger) {
            return f(re.as_ref());
        }
    }

    let compiled = Regex::new(&format!(r"(?i)\b{}\b", regex::escape(trigger))).ok();
    let mut map = match cache.write() {
        Ok(map) => map,
        // A poisoned lock only means some other thread panicked mid-write;
        // the entry is still worth using, just not worth caching.
        Err(_) => return f(compiled.as_ref()),
    };
    f(map.entry(trigger.to_string()).or_insert(compiled).as_ref())
}

fn word_re() -> &'static Regex {
    static WORD_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    WORD_RE.get_or_init(|| Regex::new(r"[a-zA-Z0-9'\-]+").unwrap())
}

fn brand_re() -> &'static Regex {
    static BRAND_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    BRAND_RE.get_or_init(|| {
        Regex::new(r"(?i)\b[a-z0-9'-]{2,}\s+(control|ctrl|ctl|kontrol)\b").unwrap()
    })
}

/// Fuzzy-corrects occurrences of `custom_vocab` entries in `text` using
/// Levenshtein distance, so phonetic mis-transcriptions/mis-readings of
/// proper nouns and domain-specific terms get normalized back to the
/// configured spelling.
///
/// Multi-word phrases are corrected first (longest phrase first, so a
/// longer match "wins" over a shorter one contained within it), followed
/// by single-word corrections. The allowed edit distance scales with word
/// length: exact match only for 1-3 chars, distance <= 1 for mid-length
/// words, distance <= 2 for longer words.
pub fn correct_custom_vocabulary(text: &str, custom_vocab: &[String]) -> String {
    let mut result = text.to_string();

    let mut multi_word: Vec<&String> = Vec::new();
    // Lowercased once here rather than once per (word, entry) comparison
    // inside the replace loop below.
    let mut single_word: Vec<(&str, String)> = Vec::new();
    for vocab_word in custom_vocab {
        if vocab_word.trim().is_empty() {
            continue;
        }
        if vocab_word.contains(' ') {
            multi_word.push(vocab_word);
        } else {
            single_word.push((vocab_word.as_str(), vocab_word.to_lowercase()));
        }
    }

    // 1. Process multi-word phrases first (longest first)
    multi_word.sort_by(|a, b| b.len().cmp(&a.len()));

    if !multi_word.is_empty() {
        let re_word = word_re();
        for phrase in multi_word {
            let phrase_lower = phrase.to_lowercase();
            let word_count = phrase_lower.split_whitespace().count();
            if word_count == 0 {
                continue;
            }
            let phrase_len = phrase_lower.chars().count();
            let max_allowed = if phrase_len <= 4 {
                0
            } else if phrase_len <= 6 {
                1
            } else {
                3
            };

            let mut replaced_any = true;
            while replaced_any {
                replaced_any = false;
                let tokens: Vec<_> = re_word.find_iter(&result).collect();
                if tokens.len() < word_count {
                    break;
                }

                for i in 0..=(tokens.len() - word_count) {
                    let start_byte = tokens[i].start();
                    let end_byte = tokens[i + word_count - 1].end();

                    let candidate_str = &result[start_byte..end_byte];
                    let candidate_words: Vec<&str> = candidate_str
                        .split_whitespace()
                        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()))
                        .filter(|w| !w.is_empty())
                        .collect();

                    if candidate_words.len() != word_count {
                        continue;
                    }

                    let candidate_norm = candidate_words.join(" ").to_lowercase();
                    if !within(&candidate_norm, &phrase_lower, max_allowed) {
                        continue;
                    }

                    if candidate_str != *phrase {
                        result.replace_range(start_byte..end_byte, phrase);
                        replaced_any = true;
                        break;
                    }
                }
            }
        }
    }

    // 2. Process single-word corrections
    if !single_word.is_empty() {
        result = word_re()
            .replace_all(&result, |caps: &regex::Captures| {
                let matched = caps.get(0).unwrap().as_str();

                let mut best_match: Option<&str> = None;
                let mut best_dist = usize::MAX;

                let matched_lower = matched.to_lowercase();

                for (vocab_word, vocab_lower) in &single_word {
                    if matched_lower == *vocab_lower {
                        best_match = Some(vocab_word);
                        break;
                    }

                    let len = vocab_lower.chars().count();
                    let max_allowed = if len <= 3 {
                        0
                    } else if len == 4 {
                        1
                    } else {
                        2
                    };
                    // Nothing under `best_dist` can be found above it either,
                    // so the search narrows as a better match is found.
                    let ceiling = max_allowed.min(best_dist.saturating_sub(1));

                    if let Some(dist) = distance_within(&matched_lower, vocab_lower, ceiling) {
                        best_dist = dist;
                        best_match = Some(vocab_word);
                    }
                }

                best_match.unwrap_or(matched).to_string()
            })
            .into_owned();
    }

    // 3. Dynamic VoxCtrl brand homophone fallback
    // Matches any remaining "<word> control/ctrl" phrase within edit distance <= 1 of "vox control"
    result = brand_re()
        .replace_all(&result, |caps: &regex::Captures| {
            let matched = caps.get(0).unwrap().as_str();
            let matched_lower = matched.to_lowercase();
            let is_already_brand =
                matched == "VoxCtrl" || matched == "Vox Ctrl" || matched_lower == "vox ctrl";
            if !is_already_brand && within(&matched_lower, "vox control", 1) {
                "VoxCtrl".to_string()
            } else {
                matched.to_string()
            }
        })
        .into_owned();

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levenshtein_identical_strings_is_zero() {
        assert_eq!(levenshtein_distance("hello", "hello"), 0);
    }

    #[test]
    fn levenshtein_basic_cases() {
        assert_eq!(levenshtein_distance("kitten", "sitting"), 3);
        assert_eq!(levenshtein_distance("", "abc"), 3);
        assert_eq!(levenshtein_distance("abc", ""), 3);
    }

    #[test]
    fn snippet_expand() {
        let mut snips = HashMap::new();
        snips.insert("addr".into(), "123 Main Street".into());
        let result = expand_snippets("Send to addr please", &snips);
        assert_eq!(result, "Send to 123 Main Street please");
    }

    #[test]
    fn snippet_expand_empty_map_is_noop() {
        let snips = HashMap::new();
        assert_eq!(expand_snippets("unchanged text", &snips), "unchanged text");
    }

    #[test]
    fn custom_vocab_correction() {
        let vocab = vec![
            "Waylin".to_string(),
            "Rufer".to_string(),
            "Enola".to_string(),
            "Kenz".to_string(),
            "Vox Ctrl".to_string(),
        ];

        // Exact case-insensitive matches should capitalize correctly
        assert_eq!(correct_custom_vocabulary("hello waylin", &vocab), "hello Waylin");
        assert_eq!(correct_custom_vocabulary("RUFER is here", &vocab), "Rufer is here");
        assert_eq!(correct_custom_vocabulary("kenz", &vocab), "Kenz");

        // Fuzzy matches (edit distance <= 2 for long words, <= 1 for mid-length)
        assert_eq!(correct_custom_vocabulary("Hello Waylan!", &vocab), "Hello Waylin!");
        assert_eq!(correct_custom_vocabulary("this is Enoll", &vocab), "this is Enola");
        assert_eq!(correct_custom_vocabulary("my friend kens", &vocab), "my friend Kenz");

        // Short words and distant words should not trigger false positives
        assert_eq!(correct_custom_vocabulary("in", &vocab), "in");
        // Normal speech phrases containing "control" must not be modified
        assert_eq!(correct_custom_vocabulary("The foxes control the hen house", &vocab), "The foxes control the hen house");
        assert_eq!(correct_custom_vocabulary("The foxes control the hen house", &[]), "The foxes control the hen house");
    }

    /// The textbook full-table implementation, kept here as the reference the
    /// rolling-row version with its early exit has to agree with.
    fn naive_distance(s1: &str, s2: &str) -> usize {
        let a: Vec<char> = s1.chars().collect();
        let b: Vec<char> = s2.chars().collect();
        let mut dp = vec![vec![0usize; b.len() + 1]; a.len() + 1];
        for (i, row) in dp.iter_mut().enumerate() {
            row[0] = i;
        }
        for j in 0..=b.len() {
            dp[0][j] = j;
        }
        for i in 1..=a.len() {
            for j in 1..=b.len() {
                dp[i][j] = if a[i - 1] == b[j - 1] {
                    dp[i - 1][j - 1]
                } else {
                    1 + dp[i - 1][j - 1].min(dp[i - 1][j]).min(dp[i][j - 1])
                };
            }
        }
        dp[a.len()][b.len()]
    }

    const SAMPLES: &[&str] = &[
        "", "a", "ab", "kitten", "sitting", "Waylin", "waylan", "vox control",
        "voxctrl", "naïve", "naive", "ÅngstrÖm", "angstrom", "a-very-long-token",
    ];

    #[test]
    fn distance_matches_the_full_table() {
        for x in SAMPLES {
            for y in SAMPLES {
                assert_eq!(
                    levenshtein_distance(x, y),
                    naive_distance(x, y),
                    "distance disagreed for {x:?} vs {y:?}"
                );
            }
        }
    }

    #[test]
    fn the_early_exit_never_changes_the_answer() {
        // `within` may stop filling the table as soon as the budget is blown;
        // whether it stops or not, its verdict has to match the real distance.
        for x in SAMPLES {
            for y in SAMPLES {
                let real = naive_distance(x, y);
                for max in 0..=6 {
                    assert_eq!(
                        within(x, y, max),
                        real <= max,
                        "{x:?} vs {y:?} at max={max} (real distance {real})"
                    );
                    assert_eq!(
                        distance_within(x, y, max),
                        (real <= max).then_some(real),
                        "{x:?} vs {y:?} at max={max}"
                    );
                }
            }
        }
    }

    #[test]
    fn custom_vocab_correction_empty_vocab_is_noop() {
        assert_eq!(correct_custom_vocabulary("hello world", &[]), "hello world");
    }
}
