use std::collections::HashMap;

use regex::Regex;

// ── Filler removal ────────────────────────────────────────────────────────────

static FILLER_PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

fn filler_re() -> &'static Regex {
    FILLER_PATTERN.get_or_init(|| {
        Regex::new(r"(?i)\b(uh+|um+|hmm+|er+|ah+|ugh+|mhm+)\b,?\s*").unwrap()
    })
}

pub fn remove_fillers(text: &str) -> String {
    filler_re().replace_all(text, "").trim().to_string()
}

// ── Spoken punctuation ────────────────────────────────────────────────────────

static PUNCT_WORDS: &[(&str, &str)] = &[
    ("period",           ". "),
    ("full stop",        ". "),
    ("comma",            ", "),
    ("question mark",    "? "),
    ("exclamation mark", "! "),
    ("exclamation point","! "),
    ("colon",            ": "),
    ("semicolon",        "; "),
    ("open bracket",     "("),
    ("close bracket",    ")"),
    ("open paren",       "("),
    ("close paren",      ")"),
    ("new line",         "\n"),
    ("new paragraph",    "\n\n"),
    ("tab",              "\t"),
    ("dash",             " — "),
    ("hyphen",           "-"),
    ("ellipsis",         "..."),
    ("slash",            "/"),
    ("backslash",        "\\"),
    ("at sign",          "@"),
    ("hash",             "#"),
    ("percent",          "%"),
    ("ampersand",        "&"),
    ("asterisk",         "*"),
    ("plus sign",        "+"),
    ("equals sign",      "="),
    ("less than",        "<"),
    ("greater than",     ">"),
];

// Compiled once at first call; reused for every subsequent transcription.
static PUNCT_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
static DOUBLE_SPACE_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
static LIST_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();

/// One alternation over every spoken-punctuation word.
///
/// A regex per word meant scanning the transcript — and rebuilding it — once
/// per entry in the table; there is nothing a second pass can match that the
/// first cannot, because every replacement is punctuation. Longest phrase
/// first, so "full stop" is not cut short by a shorter alternative starting at
/// the same place.
fn punct_re() -> &'static Regex {
    PUNCT_RE.get_or_init(|| {
        let mut words: Vec<&str> = PUNCT_WORDS.iter().map(|(word, _)| *word).collect();
        words.sort_by_key(|w| std::cmp::Reverse(w.len()));
        let alternation = words
            .iter()
            .map(|w| regex::escape(w))
            .collect::<Vec<_>>()
            .join("|");
        Regex::new(&format!(r"(?i)\b({alternation})\b")).unwrap()
    })
}

fn double_space_re() -> &'static Regex {
    DOUBLE_SPACE_RE.get_or_init(|| Regex::new(r" {2,}").unwrap())
}

fn list_re() -> &'static Regex {
    LIST_RE.get_or_init(|| {
        Regex::new(
            r"(?i)\b(first(?:ly)?|second(?:ly)?|third(?:ly)?|fourth(?:ly)?|fifth(?:ly)?|finally)\b,?\s*",
        )
        .unwrap()
    })
}

pub fn apply_spoken_punctuation(text: &str) -> String {
    let replaced = punct_re().replace_all(text, |caps: &regex::Captures| {
        let spoken = &caps[1];
        match PUNCT_WORDS
            .iter()
            .find(|(word, _)| word.eq_ignore_ascii_case(spoken))
        {
            Some((_, repl)) => std::borrow::Cow::Borrowed(*repl),
            // Unreachable: the alternation is built from this same table.
            None => std::borrow::Cow::Owned(spoken.to_string()),
        }
    });
    double_space_re().replace_all(&replaced, " ").trim().to_string()
}

// ── Auto list formatting ──────────────────────────────────────────────────────

pub fn auto_format_lists(text: &str) -> String {
    let re = list_re();
    if !re.is_match(text) {
        return text.to_string();
    }

    let mut counter = 1u32;
    re.replace_all(text, |_caps: &regex::Captures| {
            let prefix = format!("\n{}. ", counter);
            counter += 1;
            prefix
        })
        .trim()
        .to_string()
}

// ── Snippet expansion ─────────────────────────────────────────────────────────

// Shared with voxctrl-tts, which applies the same expansion to text before
// speaking it. See voxctrl-text for the implementation.
pub use voxctrl_text::expand_snippets;

// ── Code mode ─────────────────────────────────────────────────────────────────

pub fn apply_code_mode(text: &str) -> String {
    // Replace spaces between words with underscores (snake_case by default)
    // Words that look like operators are preserved
    static OPERATOR_RE: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let operator_re = OPERATOR_RE
        .get_or_init(|| Regex::new(r"\b(equals|plus|minus|times|divided by|modulo)\b").unwrap());
    let s = operator_re
        .replace_all(text, |caps: &regex::Captures| -> String {
            match caps[0].to_lowercase().as_str() {
                "equals" => "=".into(),
                "plus" => "+".into(),
                "minus" => "-".into(),
                "times" => "*".into(),
                "divided by" => "/".into(),
                "modulo" => "%".into(),
                _ => caps[0].to_string(),
            }
        })
        .to_string();

    // Convert "camel case" spoken as separate words: "my function" → "myFunction"
    // Heuristic: only when the phrase starts with a lowercase letter
    let words: Vec<&str> = s.split_whitespace().collect();
    if words.len() > 1 && words[0].chars().next().map(|c| c.is_lowercase()).unwrap_or(false) {
        let camel = words
            .iter()
            .enumerate()
            .map(|(i, w)| {
                if i == 0 {
                    w.to_string()
                } else {
                    let mut c = w.chars();
                    match c.next() {
                        None => String::new(),
                        Some(f) => f.to_uppercase().to_string() + c.as_str(),
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("");
        return camel;
    }

    s
}

// Shared with voxctrl-tts, which applies the same fuzzy correction to text
// before speaking it. See voxctrl-text for the implementation.
pub use voxctrl_text::{correct_custom_vocabulary, levenshtein_distance, normalize_brand_name};

// ── Full post-processing pipeline ─────────────────────────────────────────────

pub fn is_silence_hallucination(text: &str) -> bool {
    let cleaned = text
        .trim()
        .trim_end_matches(|c: char| c.is_ascii_punctuation())
        .trim()
        .to_lowercase();
    cleaned == "thank you" || cleaned == "thanks for watching" || cleaned == "thank you for watching"
}

/// Borrows the snippet table and vocabulary from the caller's config: both
/// are read-only here, and cloning them for every transcription copied a map
/// and a vector that had not changed since startup.
#[derive(Debug, Clone)]
pub struct PostProcessConfig<'a> {
    pub remove_fillers: bool,
    pub spoken_punctuation: bool,
    pub auto_format_lists: bool,
    pub apply_snippets: bool,
    pub snippets: &'a HashMap<String, String>,
    pub code_mode: bool,
    pub custom_vocabulary: &'a [String],
}

pub fn run_pipeline(text: &str, cfg: &PostProcessConfig) -> String {
    // Borrowed until a stage actually rewrites something, so a transcript that
    // has every feature switched off is never copied at all.
    let mut s = std::borrow::Cow::Borrowed(text);

    if cfg.remove_fillers {
        s = remove_fillers(&s).into();
    }
    if cfg.spoken_punctuation {
        s = apply_spoken_punctuation(&s).into();
    }
    if cfg.auto_format_lists {
        s = auto_format_lists(&s).into();
    }
    if cfg.apply_snippets {
        s = expand_snippets(&s, cfg.snippets).into();
    }
    if !cfg.custom_vocabulary.is_empty() {
        s = correct_custom_vocabulary(&s, cfg.custom_vocabulary).into();
    }
    // Unconditional: a wake word the recogniser split into "vox control" has
    // to be repaired before the router looks for it, and whether the user
    // configured a vocabulary has nothing to do with it. Borrows straight
    // through when there is no "control"-ish word to rewrite.
    if let std::borrow::Cow::Owned(normalized) = normalize_brand_name(&s) {
        s = std::borrow::Cow::Owned(normalized);
    }
    if cfg.code_mode {
        s = apply_code_mode(&s).into();
    }

    s.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spoken_punct_regex_cached() {
        // Calling twice must return the exact same address (OnceLock).
        let p1 = punct_re() as *const _;
        let p2 = punct_re() as *const _;
        assert_eq!(p1, p2, "punct_re must return the same compiled Regex");
    }

    #[test]
    fn test_every_spoken_punct_word_is_replaced() {
        // One alternation stands in for what used to be one regex per word, so
        // every entry in the table has to still come out as its punctuation.
        for (word, repl) in PUNCT_WORDS {
            let out = apply_spoken_punctuation(&format!("a {word} b"));
            assert!(
                !out.to_lowercase().contains(word),
                "'{word}' survived as a spoken word: {out:?}"
            );
            assert!(
                out.contains(repl.trim()),
                "'{word}' did not produce {repl:?}: {out:?}"
            );
        }
    }

    #[test]
    fn test_auto_format_lists_regex_cached() {
        let p1 = list_re() as *const _;
        let p2 = list_re() as *const _;
        assert_eq!(p1, p2, "list_re must return the same compiled Regex");
    }

    #[test]
    fn test_double_space_regex_cached() {
        let p1 = double_space_re() as *const _;
        let p2 = double_space_re() as *const _;
        assert_eq!(p1, p2, "double_space_re must return the same compiled Regex");
    }

    #[test]
    fn test_spoken_punct_correctness() {
        // The regex matches only the spoken word; the preceding space is preserved.
        assert_eq!(
            apply_spoken_punctuation("Hello period world"),
            "Hello . world"
        );
        assert_eq!(
            apply_spoken_punctuation("yes comma no"),
            "yes , no"
        );
        assert_eq!(
            apply_spoken_punctuation("what question mark"),
            "what ?"
        );
    }

    /// The regression this fixes: with no custom vocabulary — the default
    /// install — the pipeline skipped `correct_custom_vocabulary`, and the
    /// brand repair was buried inside it. A wake word the recogniser split
    /// into two words therefore reached the router unrepaired, which typed
    /// the command out instead of routing it.
    #[test]
    fn brand_repair_runs_with_no_custom_vocabulary() {
        let snippets = HashMap::new();
        let cfg = PostProcessConfig {
            remove_fillers: false,
            spoken_punctuation: false,
            auto_format_lists: false,
            apply_snippets: false,
            snippets: &snippets,
            code_mode: false,
            custom_vocabulary: &[],
        };
        assert_eq!(
            run_pipeline("vox control say hello", &cfg),
            "VoxCtrl say hello"
        );
        // And it still leaves ordinary speech alone.
        assert_eq!(
            run_pipeline("the foxes control the hen house", &cfg),
            "the foxes control the hen house"
        );
    }

    #[test]
    fn test_silence_hallucinations() {
        assert!(is_silence_hallucination("Thank you."));
        assert!(is_silence_hallucination("Thank you!"));
        assert!(is_silence_hallucination("thank you"));
        assert!(is_silence_hallucination("Thanks for watching"));
        assert!(is_silence_hallucination("Thank you for watching."));
        
        // These should NOT be matched as silence hallucinations
        assert!(!is_silence_hallucination("Thank you for your help"));
        assert!(!is_silence_hallucination("Thank you very much"));
        assert!(!is_silence_hallucination("Hello world"));
    }

    #[test]
    fn filler_removal() {
        assert_eq!(remove_fillers("Hello uh world"), "Hello world");
        assert_eq!(remove_fillers("um, this is a test"), "this is a test");
    }

    #[test]
    fn spoken_punct() {
        let result = apply_spoken_punctuation("Hello world period");
        assert!(result.contains(". ") || result.ends_with('.'));
    }

    // Full snippet-expansion / custom-vocabulary coverage now lives in
    // voxctrl-text (shared with voxctrl-tts). This is a thin smoke test
    // confirming the re-exports are wired up correctly from this module.
    #[test]
    fn snippet_and_vocab_reexports_work() {
        let mut snips = HashMap::new();
        snips.insert("addr".into(), "123 Main Street".into());
        assert_eq!(expand_snippets("Send to addr please", &snips), "Send to 123 Main Street please");

        let vocab = vec!["Rufer".to_string()];
        assert_eq!(correct_custom_vocabulary("RUFER is here", &vocab), "Rufer is here");
    }
}
