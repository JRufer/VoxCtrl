//! Rewrites digits and a few symbols as English words, for TTS models that
//! read text literally.
//!
//! Kitten TTS 2 and Reverb-TTS ship text normalizers in their Python packages, but
//! audio.cpp's native ports do not include them, and the models themselves do
//! not reliably read "$4.50", "3rd" or "75%" — so VoxCtrl spells them out
//! before the text reaches the model. Deliberately small: numbers, money,
//! percentages, ordinals, clock times and a handful of symbols. Anything it
//! doesn't recognise passes through unchanged.

use std::sync::OnceLock;

use regex::{Captures, Regex};

const ONES: [&str; 20] = [
    "zero",
    "one",
    "two",
    "three",
    "four",
    "five",
    "six",
    "seven",
    "eight",
    "nine",
    "ten",
    "eleven",
    "twelve",
    "thirteen",
    "fourteen",
    "fifteen",
    "sixteen",
    "seventeen",
    "eighteen",
    "nineteen",
];
const TENS: [&str; 10] = ["", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety"];
const SCALES: [&str; 5] = ["", "thousand", "million", "billion", "trillion"];

fn below_thousand(n: u64, out: &mut Vec<String>) {
    let (hundreds, rest) = (n / 100, n % 100);
    if hundreds > 0 {
        out.push(ONES[hundreds as usize].into());
        out.push("hundred".into());
    }
    if rest >= 20 {
        let (t, o) = (rest / 10, rest % 10);
        if o == 0 {
            out.push(TENS[t as usize].into());
        } else {
            out.push(format!("{}-{}", TENS[t as usize], ONES[o as usize]));
        }
    } else if rest > 0 {
        out.push(ONES[rest as usize].into());
    }
}

/// Cardinal words for `n`, e.g. `1204` → "one thousand two hundred four".
/// Returns `None` past the trillions, where reading digits is clearer anyway.
pub fn number_to_words(n: u64) -> Option<String> {
    if n == 0 {
        return Some("zero".into());
    }
    if n >= 1_000_000_000_000_000 {
        return None;
    }
    let mut groups = Vec::new();
    let mut rest = n;
    while rest > 0 {
        groups.push(rest % 1000);
        rest /= 1000;
    }
    let mut words = Vec::new();
    for (scale, &group) in groups.iter().enumerate().rev() {
        if group == 0 {
            continue;
        }
        below_thousand(group, &mut words);
        if !SCALES[scale].is_empty() {
            words.push(SCALES[scale].into());
        }
    }
    Some(words.join(" "))
}

/// Ordinal words for `n`, e.g. `21` → "twenty-first".
fn ordinal_words(n: u64) -> Option<String> {
    let cardinal = number_to_words(n)?;
    let (head, last) = match cardinal.rfind([' ', '-']) {
        Some(i) => cardinal.split_at(i + 1),
        None => ("", cardinal.as_str()),
    };
    let last = match last {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    };
    Some(format!("{head}{last}"))
}

/// Digits read one by one, for the part after a decimal point.
fn digit_words(digits: &str) -> String {
    digits.chars().filter_map(|c| c.to_digit(10)).map(|d| ONES[d as usize]).collect::<Vec<_>>().join(" ")
}

fn parse_grouped(s: &str) -> Option<u64> {
    s.replace(',', "").parse().ok()
}

fn cardinal_text(int: &str, frac: Option<&str>) -> Option<String> {
    let whole = number_to_words(parse_grouped(int)?)?;
    Some(match frac {
        Some(f) if !f.is_empty() => format!("{whole} point {}", digit_words(f)),
        _ => whole,
    })
}

fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        one.to_string()
    } else {
        many.to_string()
    }
}

fn money_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([$£€])(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d{1,2}))?\b").unwrap())
}

fn percent_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d+))?\s?%").unwrap())
}

fn ordinal_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(\d+)(?i:st|nd|rd|th)\b").unwrap())
}

fn time_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b([01]?\d|2[0-3]):([0-5]\d)\b").unwrap())
}

fn number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // A minus sign only after a space or other non-word character, so a
    // hyphen joining two words or numbers ("x-1", "2-3") stays a hyphen —
    // the number after it then matches with the hyphen as its prefix.
    RE.get_or_init(|| Regex::new(r"(^|[^\w])(-?)(\d{1,3}(?:,\d{3})+|\d+)(?:\.(\d+))?\b").unwrap())
}

fn money(c: &Captures) -> String {
    let (unit_one, unit_many, sub_one, sub_many) = match &c[1] {
        "£" => ("pound", "pounds", "penny", "pence"),
        "€" => ("euro", "euros", "cent", "cents"),
        _ => ("dollar", "dollars", "cent", "cents"),
    };
    let Some(whole) = parse_grouped(&c[2]) else { return c[0].to_string() };
    let Some(whole_words) = number_to_words(whole) else { return c[0].to_string() };
    let mut out = format!("{whole_words} {}", plural(whole, unit_one, unit_many));
    if let Some(sub) = c.get(3) {
        // "$4.5" means fifty cents, not five.
        let cents: u64 = format!("{:0<2}", sub.as_str()).parse().unwrap_or(0);
        if cents > 0 {
            let cents_words = number_to_words(cents).unwrap_or_default();
            out = format!("{out} and {cents_words} {}", plural(cents, sub_one, sub_many));
        }
    }
    out
}

/// Spells out numbers, money, percentages, ordinals and clock times in
/// `text` as English words, and reads `&` as "and". Text with no digits
/// comes back unchanged.
pub fn spell_out_numbers_en(text: &str) -> String {
    let text = text.replace(" & ", " and ");
    if !text.bytes().any(|b| b.is_ascii_digit()) {
        return text;
    }
    let text = money_re().replace_all(&text, |c: &Captures| money(c));
    let text =
        percent_re().replace_all(&text, |c: &Captures| match cardinal_text(&c[1], c.get(2).map(|m| m.as_str())) {
            Some(words) => format!("{words} percent"),
            None => c[0].to_string(),
        });
    let text = ordinal_re().replace_all(&text, |c: &Captures| {
        c[1].parse().ok().and_then(ordinal_words).unwrap_or_else(|| c[0].to_string())
    });
    let text = time_re().replace_all(&text, |c: &Captures| {
        let hour: u64 = c[1].parse().unwrap_or(0);
        let minute: u64 = c[2].parse().unwrap_or(0);
        let hour_words = number_to_words(hour).unwrap_or_default();
        match minute {
            0 => format!("{hour_words} o'clock"),
            1..=9 => format!("{hour_words} oh {}", ONES[minute as usize]),
            _ => format!("{hour_words} {}", number_to_words(minute).unwrap_or_default()),
        }
    });
    let text =
        number_re().replace_all(&text, |c: &Captures| match cardinal_text(&c[3], c.get(4).map(|m| m.as_str())) {
            Some(words) => {
                let sign = if &c[2] == "-" { "minus " } else { "" };
                format!("{}{sign}{words}", &c[1])
            }
            None => c[0].to_string(),
        });
    text.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cardinals() {
        assert_eq!(number_to_words(0).unwrap(), "zero");
        assert_eq!(number_to_words(7).unwrap(), "seven");
        assert_eq!(number_to_words(40).unwrap(), "forty");
        assert_eq!(number_to_words(42).unwrap(), "forty-two");
        assert_eq!(number_to_words(100).unwrap(), "one hundred");
        assert_eq!(number_to_words(1204).unwrap(), "one thousand two hundred four");
        assert_eq!(number_to_words(2_000_015).unwrap(), "two million fifteen");
        assert!(number_to_words(u64::MAX).is_none());
    }

    #[test]
    fn ordinals() {
        assert_eq!(ordinal_words(1).unwrap(), "first");
        assert_eq!(ordinal_words(12).unwrap(), "twelfth");
        assert_eq!(ordinal_words(20).unwrap(), "twentieth");
        assert_eq!(ordinal_words(21).unwrap(), "twenty-first");
        assert_eq!(ordinal_words(103).unwrap(), "one hundred third");
    }

    #[test]
    fn plain_text_is_untouched() {
        let s = "Hello there, how are you?";
        assert_eq!(spell_out_numbers_en(s), s);
    }

    #[test]
    fn numbers_in_sentences() {
        assert_eq!(spell_out_numbers_en("I have 3 cats."), "I have three cats.");
        assert_eq!(spell_out_numbers_en("About 1,500 people"), "About one thousand five hundred people");
        assert_eq!(spell_out_numbers_en("It is 3.14 wide"), "It is three point one four wide");
        assert_eq!(spell_out_numbers_en("It was -5 outside"), "It was minus five outside");
    }

    #[test]
    fn hyphenated_numbers_keep_their_hyphen() {
        assert_eq!(spell_out_numbers_en("a 2-3 split"), "a two-three split");
        assert_eq!(spell_out_numbers_en("model x-1"), "model x-one");
    }

    #[test]
    fn money_percent_ordinal_time() {
        assert_eq!(spell_out_numbers_en("It costs $4.50"), "It costs four dollars and fifty cents");
        assert_eq!(spell_out_numbers_en("$1"), "one dollar");
        assert_eq!(spell_out_numbers_en("£2.01"), "two pounds and one penny");
        assert_eq!(spell_out_numbers_en("75% done"), "seventy-five percent done");
        assert_eq!(spell_out_numbers_en("the 3rd time"), "the third time");
        assert_eq!(spell_out_numbers_en("at 3:05 or 14:30"), "at three oh five or fourteen thirty");
        assert_eq!(spell_out_numbers_en("at 9:00"), "at nine o'clock");
    }

    #[test]
    fn ampersand_and_tags_survive() {
        assert_eq!(
            spell_out_numbers_en("[excited] Tom & Jerry <laugh> won 2 prizes"),
            "[excited] Tom and Jerry <laugh> won two prizes"
        );
    }

    #[test]
    fn digits_inside_words_are_left_alone() {
        // Model names, versions and IDs read better as written.
        assert_eq!(spell_out_numbers_en("run mp3 files"), "run mp3 files");
    }
}
