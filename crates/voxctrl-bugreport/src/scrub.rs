//! Text scrubbing applied to every string that survives into a report.
//!
//! Two things leak through strings that were never meant to carry them: the
//! path to the user's home directory (which on every platform contains their
//! account name) and the account name itself, which turns up in device names,
//! error messages and log lines. Both are stripped from every value the report
//! keeps, on top of whatever the per-field rules in [`crate::redact`] already
//! do — belt and braces, because the field rules can only know about fields
//! that exist today, and a log line is not a field at all.

/// The identifiers to remove from report text.
///
/// Built once and passed down rather than read from the environment inside the
/// scrubber, so the tests can exercise it with a home directory and a user name
/// that are not the ones running the test suite.
#[derive(Debug, Clone, Default)]
pub struct Scrubber {
    home: Option<String>,
    user: Option<String>,
}

impl Scrubber {
    /// Read the current user's home directory and account name.
    pub fn from_env() -> Self {
        let home = dirs::home_dir().map(|p| p.to_string_lossy().into_owned());
        // USERNAME is what Windows sets; USER is what a POSIX shell sets. The
        // last path component of the home directory is the fallback, because a
        // GUI session started by the desktop may export neither.
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .ok()
            .filter(|u| !u.is_empty())
            .or_else(|| {
                dirs::home_dir()
                    .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
            });
        Self::new(home, user)
    }

    pub fn new(home: Option<String>, user: Option<String>) -> Self {
        // A one- or two-character account name would match half the text in the
        // report, so it is left alone: replacing it would destroy the report
        // without hiding anything a three-letter string was ever hiding.
        let user = user.filter(|u| u.chars().count() >= 3);
        let home = home.filter(|h| !h.is_empty() && h != "/");
        Self { home, user }
    }

    /// Replace the home directory with `~` and the account name with `<user>`.
    ///
    /// Both replacements are case-insensitive: Windows paths come back from
    /// different APIs with different capitalisation of the same directory, and
    /// a case-sensitive pass would miss `C:\USERS\Jane` while catching
    /// `C:\Users\Jane`.
    pub fn scrub(&self, text: &str) -> String {
        let mut out = text.to_string();
        if let Some(home) = &self.home {
            out = replace_ignore_case(&out, home, "~");
            // Windows paths reach us in both slash conventions; the JSON in a
            // config file usually carries forward slashes even on Windows.
            let flipped: String = home.replace('\\', "/");
            if flipped != *home {
                out = replace_ignore_case(&out, &flipped, "~");
            }
        }
        if let Some(user) = &self.user {
            out = replace_ignore_case(&out, user, "<user>");
        }
        out
    }
}

/// `str::replace`, but matching without regard to case.
fn replace_ignore_case(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() {
        return haystack.to_string();
    }
    let lower_hay = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();

    // Lowercasing can change a string's byte length (ẞ → ss), which would make
    // indices from the lowered copy meaningless against the original. Both of
    // our needles are paths and account names, so this is vanishingly rare —
    // but "vanishingly rare" is not "never", and getting it wrong would splice
    // a report mid-character. Fall back to an exact match instead.
    if lower_hay.len() != haystack.len() {
        return haystack.replace(needle, replacement);
    }

    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0usize;
    while let Some(found) = lower_hay[cursor..].find(&lower_needle) {
        let start = cursor + found;
        let end = start + lower_needle.len();
        if !haystack.is_char_boundary(start) || !haystack.is_char_boundary(end) {
            break;
        }
        out.push_str(&haystack[cursor..start]);
        out.push_str(replacement);
        cursor = end;
    }
    out.push_str(&haystack[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrubber() -> Scrubber {
        Scrubber::new(Some("/home/jane".into()), Some("jane".into()))
    }

    #[test]
    fn the_home_directory_becomes_a_tilde() {
        assert_eq!(
            scrubber().scrub("model dir is /home/jane/.local/share/voxctrl/models"),
            "model dir is ~/.local/share/voxctrl/models"
        );
    }

    #[test]
    fn an_account_name_outside_a_path_is_replaced_too() {
        // The account name turns up in places that are not paths at all —
        // an ALSA device called "jane's headset", a log line naming a D-Bus
        // connection. The home-directory rule alone would miss every one.
        assert_eq!(
            scrubber().scrub("device: jane's headset"),
            "device: <user>'s headset"
        );
    }

    #[test]
    fn windows_paths_are_matched_whatever_their_capitalisation() {
        let s = Scrubber::new(Some(r"C:\Users\Jane".into()), Some("Jane".into()));
        assert_eq!(
            s.scrub(r"failed to open C:\USERS\JANE\AppData\Local\voxctrl"),
            r"failed to open ~\AppData\Local\voxctrl"
        );
    }

    #[test]
    fn a_windows_home_is_matched_with_forward_slashes_as_well() {
        // Config files written by the app carry forward slashes even on
        // Windows, so the same directory has to be caught in both spellings.
        let s = Scrubber::new(Some(r"C:\Users\Jane".into()), Some("Jane".into()));
        assert_eq!(s.scrub("C:/Users/Jane/voices"), "~/voices");
    }

    #[test]
    fn a_very_short_account_name_is_left_alone() {
        // "jo" appears inside "join", "major", "jog". Replacing it would
        // shred the report and hide nothing that two letters were hiding.
        let s = Scrubber::new(Some("/home/jo".into()), Some("jo".into()));
        assert_eq!(s.scrub("failed to join the audio thread"), "failed to join the audio thread");
        // The home directory is still stripped; only the bare-name rule is off.
        assert_eq!(s.scrub("/home/jo/models"), "~/models");
    }

    #[test]
    fn nothing_known_means_nothing_changed() {
        let s = Scrubber::new(None, None);
        assert_eq!(s.scrub("/home/jane/x"), "/home/jane/x");
    }
}
