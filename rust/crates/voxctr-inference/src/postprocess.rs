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

pub fn apply_spoken_punctuation(text: &str) -> String {
    let mut result = text.to_string();
    for (word, replacement) in PUNCT_WORDS {
        // Case-insensitive word-boundary replacement
        let pattern = format!(r"(?i)\b{}\b", regex::escape(word));
        if let Ok(re) = Regex::new(&pattern) {
            result = re.replace_all(&result, *replacement).to_string();
        }
    }
    // Clean up double spaces
    let ws = Regex::new(r" {2,}").unwrap();
    ws.replace_all(&result, " ").trim().to_string()
}

// ── Auto list formatting ──────────────────────────────────────────────────────

pub fn auto_format_lists(text: &str) -> String {
    // If text contains "first ... second ... third" pattern, format as list
    let list_re = Regex::new(
        r"(?i)\b(first(?:ly)?|second(?:ly)?|third(?:ly)?|fourth(?:ly)?|fifth(?:ly)?|finally)\b,?\s*"
    ).unwrap();
    if !list_re.is_match(text) {
        return text.to_string();
    }

    let mut counter = 1u32;
    list_re
        .replace_all(text, |_caps: &regex::Captures| {
            let prefix = format!("\n{}. ", counter);
            counter += 1;
            prefix
        })
        .trim()
        .to_string()
}

// ── Snippet expansion ─────────────────────────────────────────────────────────

pub fn expand_snippets(text: &str, snippets: &HashMap<String, String>) -> String {
    if snippets.is_empty() {
        return text.to_string();
    }
    let mut result = text.to_string();
    for (trigger, expansion) in snippets {
        let pattern = format!(r"(?i)\b{}\b", regex::escape(trigger));
        if let Ok(re) = Regex::new(&pattern) {
            result = re.replace_all(&result, expansion.as_str()).to_string();
        }
    }
    result
}

// ── Code mode ─────────────────────────────────────────────────────────────────

pub fn apply_code_mode(text: &str) -> String {
    // Replace spaces between words with underscores (snake_case by default)
    // Words that look like operators are preserved
    let operator_re = Regex::new(r"\b(equals|plus|minus|times|divided by|modulo)\b").unwrap();
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

// ── Full post-processing pipeline ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PostProcessConfig {
    pub remove_fillers: bool,
    pub spoken_punctuation: bool,
    pub auto_format_lists: bool,
    pub apply_snippets: bool,
    pub snippets: HashMap<String, String>,
    pub code_mode: bool,
}

pub fn run_pipeline(text: &str, cfg: &PostProcessConfig) -> String {
    let mut s = text.to_string();

    if cfg.remove_fillers {
        s = remove_fillers(&s);
    }
    if cfg.spoken_punctuation {
        s = apply_spoken_punctuation(&s);
    }
    if cfg.auto_format_lists {
        s = auto_format_lists(&s);
    }
    if cfg.apply_snippets {
        s = expand_snippets(&s, &cfg.snippets);
    }
    if cfg.code_mode {
        s = apply_code_mode(&s);
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn snippet_expand() {
        let mut snips = HashMap::new();
        snips.insert("addr".into(), "123 Main Street".into());
        let result = expand_snippets("Send to addr please", &snips);
        assert_eq!(result, "Send to 123 Main Street please");
    }
}
