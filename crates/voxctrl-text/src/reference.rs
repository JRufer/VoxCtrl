//! Reference copies of the pre-optimization implementations, kept only for the
//! differential test below. Verbatim from 6195ee0.
#![allow(dead_code)]
use std::collections::HashMap;
use regex::Regex;

/// Classic Levenshtein (edit) distance between two strings, computed over
/// `char`s rather than bytes so it works correctly with non-ASCII input.
pub fn ref_levenshtein_distance(s1: &str, s2: &str) -> usize {
    let s1_chars: Vec<char> = s1.chars().collect();
    let s2_chars: Vec<char> = s2.chars().collect();
    let len1 = s1_chars.len();
    let len2 = s2_chars.len();

    let mut dp = vec![vec![0; len2 + 1]; len1 + 1];

    for i in 0..=len1 {
        dp[i][0] = i;
    }
    for j in 0..=len2 {
        dp[0][j] = j;
    }

    for i in 1..=len1 {
        for j in 1..=len2 {
            if s1_chars[i - 1] == s2_chars[j - 1] {
                dp[i][j] = dp[i - 1][j - 1];
            } else {
                dp[i][j] = 1 + std::cmp::min(
                    dp[i - 1][j - 1], // substitution
                    std::cmp::min(
                        dp[i - 1][j], // deletion
                        dp[i][j - 1], // insertion
                    )
                );
            }
        }
    }

    dp[len1][len2]
}

/// Replaces short trigger words/phrases in `text` with their configured
/// expansions (case-insensitive, word-boundary matched). Used for both
/// dictation snippets (STT output) and spoken-text shortcuts (TTS input).
pub fn ref_expand_snippets(text: &str, snippets: &HashMap<String, String>) -> String {
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
pub fn ref_correct_custom_vocabulary(text: &str, custom_vocab: &[String]) -> String {
    let mut result = text.to_string();

    let mut multi_word: Vec<&String> = Vec::new();
    let mut single_word: Vec<&String> = Vec::new();
    for vocab_word in custom_vocab {
        if vocab_word.trim().is_empty() {
            continue;
        }
        if vocab_word.contains(' ') {
            multi_word.push(vocab_word);
        } else {
            single_word.push(vocab_word);
        }
    }

    // 1. Process multi-word phrases first (longest first)
    multi_word.sort_by(|a, b| b.len().cmp(&a.len()));

    if !multi_word.is_empty() {
        let re_word = match Regex::new(r"[a-zA-Z0-9'\-]+") {
            Ok(re) => re,
            Err(_) => return result,
        };
        for phrase in multi_word {
            let phrase_lower = phrase.to_lowercase();
            let phrase_words: Vec<&str> = phrase_lower.split_whitespace().collect();
            let n = phrase_words.len();
            if n == 0 {
                continue;
            }

            let mut replaced_any = true;
            while replaced_any {
                replaced_any = false;
                let tokens: Vec<_> = re_word.find_iter(&result).collect();
                if tokens.len() < n {
                    break;
                }

                for i in 0..=(tokens.len() - n) {
                    let start_tok = &tokens[i];
                    let end_tok = &tokens[i + n - 1];
                    let start_byte = start_tok.start();
                    let end_byte = end_tok.end();

                    let candidate_str = &result[start_byte..end_byte];
                    let candidate_words: Vec<&str> = candidate_str
                        .split_whitespace()
                        .map(|w| w.trim_matches(|c: char| c.is_ascii_punctuation()))
                        .filter(|w| !w.is_empty())
                        .collect();

                    if candidate_words.len() != n {
                        continue;
                    }

                    let candidate_norm = candidate_words.join(" ").to_lowercase();
                    let dist = ref_levenshtein_distance(&candidate_norm, &phrase_lower);

                    let phrase_len = phrase_lower.chars().count();
                    let max_allowed = if phrase_len <= 4 {
                        0
                    } else if phrase_len <= 6 {
                        1
                    } else {
                        3
                    };

                    if dist <= max_allowed {
                        if candidate_str != *phrase {
                            result.replace_range(start_byte..end_byte, phrase);
                            replaced_any = true;
                            break;
                        }
                    }
                }
            }
        }
    }

    // 2. Process single-word corrections
    if !single_word.is_empty() {
        let re_word = match Regex::new(r"[a-zA-Z0-9'\-]+") {
            Ok(re) => re,
            Err(_) => return result,
        };

        result = re_word.replace_all(&result, |caps: &regex::Captures| {
            let matched = caps.get(0).unwrap().as_str();

            let mut best_match: Option<&str> = None;
            let mut best_dist = usize::MAX;

            let matched_lower = matched.to_lowercase();

            for vocab_word in &single_word {
                let vocab_lower = vocab_word.to_lowercase();
                let len = vocab_lower.chars().count();

                if matched_lower == vocab_lower {
                    best_match = Some(vocab_word.as_str());
                    break;
                }

                let dist = ref_levenshtein_distance(&matched_lower, &vocab_lower);

                let max_allowed = if len <= 3 {
                    0
                } else if len == 4 {
                    1
                } else {
                    2
                };

                if dist <= max_allowed && dist < best_dist {
                    best_dist = dist;
                    best_match = Some(vocab_word.as_str());
                }
            }

            if let Some(replacement) = best_match {
                replacement.to_string()
            } else {
                matched.to_string()
            }
        }).to_string();
    }

    // 3. Dynamic VoxCtrl brand homophone fallback
    // Matches any remaining "<word> control/ctrl" phrase within edit distance <= 3 of "vox control"
    if let Ok(re_ctrl) = Regex::new(r"(?i)\b[a-z0-9'-]{2,}\s+(control|ctrl|ctl|kontrol)\b") {
        result = re_ctrl.replace_all(&result, |caps: &regex::Captures| {
            let matched = caps.get(0).unwrap().as_str();
            let matched_lower = matched.to_lowercase();
            let is_already_brand = matched == "VoxCtrl" || matched == "Vox Ctrl" || matched_lower == "vox ctrl";
            let dist = ref_levenshtein_distance(&matched_lower, "vox control");
            if !is_already_brand && dist <= 1 {
                "VoxCtrl".to_string()
            } else {
                matched.to_string()
            }
        }).to_string();
    }

    result
}


#[cfg(test)]
mod differential {
    use super::*;
    use crate::{correct_custom_vocabulary, expand_snippets, levenshtein_distance, normalize_brand_name};

    /// Deterministic pseudo-random corpus generator.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick<T: Copy>(&mut self, xs: &[T]) -> T {
            xs[(self.next() % xs.len() as u64) as usize]
        }
        fn pick_owned(&mut self, xs: &[String]) -> String {
            xs[(self.next() % xs.len() as u64) as usize].clone()
        }
    }

    const WORDS: &[&str] = &[
        "VoxCtrl", "Voxctrl", "voxctrl", "vox", "control", "ctrl", "say", "Say", "hello",
        "Waylin", "waylan", "Rufer", "notes", "note", "the", "a", "kenz", "Kenz", "Enola",
        "hi", "send", "to", "my", "Vox", "Control", "kontrol", "naive", "naïve",
    ];
    const PUNCT: &[&str] = &["", ".", ",", "!", "?", ". "];

    fn corpus(rng: &mut Rng, n: usize) -> String {
        let mut s = String::new();
        for i in 0..n {
            if i > 0 {
                s.push(' ');
            }
            s.push_str(rng.pick(WORDS));
            s.push_str(rng.pick(PUNCT));
        }
        s
    }

    /// The optimized vocabulary correction must agree with the original on
    /// every input, including the vocabulary-driven paths the earlier
    /// end-to-end comparison never reached.
    #[test]
    fn vocabulary_correction_matches_the_original() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        let vocab_pool: Vec<String> = [
            "VoxCtrl", "Waylin", "Rufer", "Enola", "Kenz", "Vox Ctrl", "Vox Control",
            "say", "Say", "notes", "hello",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        for _ in 0..4000 {
            let n = 1 + (rng.next() % 8) as usize;
            let text = corpus(&mut rng, n);
            let vocab_len = (rng.next() % 5) as usize;
            let vocab: Vec<String> = (0..vocab_len).map(|_| rng.pick_owned(&vocab_pool)).collect();

            // The brand normalisation used to be the tail of this function and
            // is now its own; applied in the same order, the pair has to come
            // out exactly where the original did.
            let mine =
                normalize_brand_name(&correct_custom_vocabulary(&text, &vocab)).into_owned();
            let theirs = ref_correct_custom_vocabulary(&text, &vocab);
            assert_eq!(
                mine, theirs,
                "vocabulary correction diverged\n  text:  {text:?}\n  vocab: {vocab:?}"
            );
        }
    }

    #[test]
    fn snippet_expansion_matches_the_original() {
        let mut rng = Rng(0x2545_F491_4F6C_DD1D);
        for _ in 0..2000 {
            let n = 1 + (rng.next() % 8) as usize;
            let text = corpus(&mut rng, n);
            let mut snippets = HashMap::new();
            for _ in 0..(rng.next() % 3) {
                snippets.insert(rng.pick(WORDS).to_string(), rng.pick(WORDS).to_string());
            }
            assert_eq!(
                expand_snippets(&text, &snippets),
                ref_expand_snippets(&text, &snippets),
                "snippet expansion diverged\n  text: {text:?}\n  snippets: {snippets:?}"
            );
        }
    }

    #[test]
    fn distance_matches_the_original() {
        let mut rng = Rng(0xDEAD_BEEF_CAFE_F00D);
        for _ in 0..4000 {
            let a = rng.pick(WORDS);
            let b = rng.pick(WORDS);
            assert_eq!(
                levenshtein_distance(a, b),
                ref_levenshtein_distance(a, b),
                "distance diverged for {a:?} vs {b:?}"
            );
        }
    }
}
